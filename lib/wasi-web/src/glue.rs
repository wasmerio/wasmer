/// ^1: bindgen glue marks its calls as unsafe - namely the use of
///     shared references that can be sent to is not in line with
///     the way the rust borrow checker is meant to work. hence
///     this file has some `unsafe` code in it
use std::{collections::HashMap, sync::Arc};

use tokio::sync::mpsc;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use virtual_net::{UnsupportedVirtualNetworking, VirtualNetworking};
use wasm_bindgen::{prelude::*, JsCast};
use wasmer_wasix::{
    capabilities::Capabilities,
    os::{Console, InputEvent, Tty, TtyOptions},
    Pipe,
};
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};
#[allow(unused_imports)]
use xterm_js_rs::addons::fit::FitAddon;
#[allow(unused_imports)]
use xterm_js_rs::addons::web_links::WebLinksAddon;
#[allow(unused_imports)]
use xterm_js_rs::addons::webgl::WebglAddon;
use xterm_js_rs::{LogLevel, OnKeyEvent, Terminal, TerminalOptions, Theme};

use super::{common::*, pool::*};
use crate::{
    net::connect_networking,
    runtime::{TermStdout, TerminalCommandRx, WebRuntime},
};

#[macro_export]
#[doc(hidden)]
macro_rules! csi {
    ($( $l:expr ),*) => { concat!("\x1B[", $( $l ),*) };
}

#[wasm_bindgen(start)]
pub fn main() {
    //let _ = console_log::init_with_level(log::Level::Debug);
    set_panic_hook();
}

#[derive(Debug, Clone, Default)]
pub struct StartArgs {
    init: Option<String>,
    uses: Vec<String>,
    prompt: Option<String>,
    no_welcome: bool,
    connect: Option<String>,
    token: Option<String>,
}
impl StartArgs {
    pub fn parse(mut self, args: &str) -> Self {
        let query_pairs = || form_urlencoded::parse(args.as_bytes());

        let find = |key| {
            query_pairs()
                .filter(|(k, _)| k == key)
                .map(|a| a.1)
                .filter(|a| a != "undefined")
                .next()
        };

        if let Some(val) = find("init") {
            self.init = Some(val.to_string());
        }
        if let Some(val) = find("uses") {
            self.uses = val.split(",").map(|v| v.to_string()).collect();
        }
        if let Some(val) = find("prompt") {
            self.prompt = Some(val.to_string());
        }
        if let Some(val) = find("connect") {
            self.connect = Some(val.to_string());
        }
        if let Some(val) = find("no_welcome") {
            match val.as_ref() {
                "true" | "yes" | "" => self.no_welcome = true,
                _ => {}
            }
        }
        if let Some(val) = find("token") {
            self.token = Some(val.to_string());
        }
        self
    }
}

#[wasm_bindgen]
pub fn start(encoded_args: String) -> Result<(), JsValue> {
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = navigator, js_name = userAgent)]
        static USER_AGENT: String;
    }

    //ate::log_init(0i32, false);
    tracing_wasm::set_as_global_default_with_config(
        tracing_wasm::WASMLayerConfigBuilder::new()
            .set_report_logs_in_timings(false)
            .set_max_level(tracing::Level::TRACE)
            .build(),
    );

    info!("glue::start");

    let terminal = Terminal::new(
        TerminalOptions::new()
            .with_log_level(LogLevel::Info)
            .with_rows(50)
            .with_cursor_blink(true)
            .with_cursor_width(10)
            .with_font_size(16u32)
            .with_draw_bold_text_in_bright_colors(true)
            .with_right_click_selects_word(true)
            .with_theme(&Theme::new()),
    );

    let window = web_sys::window().unwrap();

    // HACK: Make the xterm terminal publicly accessible so integration tests
    // can tap into it
    js_sys::Reflect::set(&window, &JsValue::from_str("xterm"), &terminal)?;

    let location = window.location().href().unwrap();

    let user_agent = USER_AGENT.clone();
    let is_mobile = wasmer_wasix::os::common::is_mobile(&user_agent);
    debug!("user_agent: {}", user_agent);

    // Compute the configuration
    let mut args = StartArgs::default().parse(&encoded_args);
    let location = url::Url::parse(location.as_str()).unwrap();
    if let Some(query) = location.query() {
        args = args.parse(query);
    }

    let elem = window
        .document()
        .unwrap()
        .get_element_by_id("terminal")
        .unwrap();

    terminal.open(elem.clone().dyn_into()?);

    let (term_tx, mut term_rx) = mpsc::unbounded_channel();
    {
        let terminal = terminal.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let terminal: Terminal = terminal.dyn_into().unwrap();
            while let Some(cmd) = term_rx.recv().await {
                match cmd {
                    TerminalCommandRx::Print(text) => {
                        terminal.write(text.as_str());
                    }
                    TerminalCommandRx::Cls => {
                        terminal.clear();
                    }
                }
            }
        });
    }

    let front_buffer = window
        .document()
        .unwrap()
        .get_element_by_id("frontBuffer")
        .unwrap();
    let front_buffer: HtmlCanvasElement = front_buffer
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| ())
        .unwrap();
    let webgl2 = front_buffer
        .get_context("webgl2")?
        .unwrap()
        .dyn_into::<WebGl2RenderingContext>()?;

    let pool = WebThreadPool::new_with_max_threads().unwrap();
    let tty_options = TtyOptions::default();

    let (stdin_tx, stdin_rx) = Pipe::channel();
    let stdout = TermStdout::new(term_tx, tty_options.clone());
    let stderr = stdout.clone();

    let mut net: Arc<dyn VirtualNetworking + Send + Sync + 'static> =
        Arc::new(UnsupportedVirtualNetworking::default());
    if let Some(connect) = args.connect {
        net = Arc::new(connect_networking(connect))
    }

    let runtime = Arc::new(WebRuntime::new(
        pool.clone(),
        tty_options.clone(),
        webgl2,
        net,
    ));
    let mut tty = Tty::new(
        Box::new(stdin_tx),
        Box::new(stdout.clone()),
        is_mobile,
        tty_options,
    );

    let init = args.init.ok_or(JsValue::from_str(
        "no initialization package has been specified",
    ))?;
    let prompt = args
        .prompt
        .ok_or(JsValue::from_str("no prompt has been specified"))?;

    let mut console = Console::new(init.as_str(), runtime.clone())
        .with_no_welcome(args.no_welcome)
        .with_prompt(prompt);

    console = console.with_uses(args.uses);

    if let Some(token) = args.token {
        console = console.with_token(token);
    }

    let mut env = HashMap::new();
    if let Some(origin) = location.domain().clone() {
        env.insert("ORIGIN".to_string(), origin.to_string());
    }
    env.insert("LOCATION".to_string(), location.to_string());

    console = console
        .with_user_agent(user_agent.as_str())
        .with_stdin(Box::new(stdin_rx))
        .with_stdout(Box::new(stdout))
        .with_stderr(Box::new(stderr))
        .with_env(env);

    let mut capabilities = Capabilities::default();
    capabilities.threading.max_threads = Some(50);
    capabilities.threading.enable_asynchronous_threading = false;
    console = console.with_capabilities(capabilities);

    let (tx, mut rx) = mpsc::unbounded_channel();

    let tx_key = tx.clone();
    let callback = {
        Closure::wrap(Box::new(move |_e: OnKeyEvent| {
            //let event = e.dom_event();
            tx_key.send(InputEvent::Key).unwrap();
        }) as Box<dyn FnMut(_)>)
    };
    terminal.on_key(callback.as_ref().unchecked_ref());
    callback.forget();

    let tx_data = tx.clone();
    let callback = {
        Closure::wrap(Box::new(move |data: String| {
            tx_data.send(InputEvent::Data(data)).unwrap();
        }) as Box<dyn FnMut(_)>)
    };
    terminal.on_data(callback.as_ref().unchecked_ref());
    callback.forget();

    /*
    {
        let addon = FitAddon::new();
        terminal.load_addon(addon.clone().dyn_into::<FitAddon>()?.into());
        addon.fit();
    }
    */

    /*
    {
        let addon = WebLinksAddon::new();
        terminal.load_addon(addon.clone().dyn_into::<WebLinksAddon>()?.into());
        addon.fit();
    }
    */

    /*
    {
        let addon = WebglAddon::new(None);
        terminal.load_addon(addon.clone().dyn_into::<WebglAddon>()?.into());
    }
    */

    {
        let front_buffer: HtmlCanvasElement = front_buffer.clone().dyn_into().unwrap();
        let terminal: Terminal = terminal.clone().dyn_into().unwrap();

        // See ^1 at file header
        #[allow(unused_unsafe)]
        unsafe {
            term_fit(terminal, front_buffer)
        };
    }

    {
        let tty_options = tty.options();
        let front_buffer: HtmlCanvasElement = front_buffer.clone().dyn_into().unwrap();
        let terminal: Terminal = terminal.clone().dyn_into().unwrap();
        let closure = {
            Closure::wrap(Box::new(move || {
                let front_buffer: HtmlCanvasElement = front_buffer.clone().dyn_into().unwrap();
                let terminal: Terminal = terminal.clone().dyn_into().unwrap();
                // See ^1 at file header
                #[allow(unused_unsafe)]
                unsafe {
                    term_fit(
                        terminal.clone().dyn_into().unwrap(),
                        front_buffer.clone().dyn_into().unwrap(),
                    );
                }

                let tty_options = tty_options.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let cols = terminal.get_cols();
                    let rows = terminal.get_rows();
                    tty_options.set_cols(cols);
                    tty_options.set_rows(rows);
                });
            }) as Box<dyn FnMut()>)
        };
        window.add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref())?;
        window.add_event_listener_with_callback(
            "orientationchange",
            closure.as_ref().unchecked_ref(),
        )?;
        closure.forget();
    }

    terminal.focus();

    // hook the stdin to a TTY (which will have access to the terminal object)
    wasm_bindgen_futures::spawn_local(async move {
        // See ^1 at file header
        #[allow(unused_unsafe)]
        unsafe {
            crate::glue::show_terminal()
        };

        let (run_tx, mut run_rx) = tokio::sync::mpsc::channel(1);
        runtime.pool.spawn_dedicated(Box::new(move || {
            let (_, process) = console.run().unwrap();

            tty.set_signaler(Box::new(process.clone()));
            let _ = run_tx.blocking_send((tty, console));
        }));
        let (mut tty, console) = run_rx.recv().await.unwrap();

        while let Some(event) = rx.recv().await {
            tty = tty.on_event(event).await;
        }

        drop(tty);
        drop(console);
    });

    Ok(())
}

#[wasm_bindgen(module = "/js/fit.ts")]
extern "C" {
    #[wasm_bindgen(js_name = "termFit")]
    fn term_fit(terminal: Terminal, front: HtmlCanvasElement);
}

#[wasm_bindgen(module = "/js/gl.js")]
extern "C" {
    #[wasm_bindgen(js_name = "showTerminal")]
    pub fn show_terminal();
    #[wasm_bindgen(js_name = "showCanvas")]
    pub fn show_canvas();
}
