/// ^1: bindgen glue marks its calls as unsafe - namely the use of
///     shared references that can be sent to is not in line with
///     the way the rust borrow checker is meant to work. hence
///     this file has some `unsafe` code in it
use std::{collections::HashMap, sync::Arc};

use tokio::sync::mpsc;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use wasm_bindgen::{prelude::*, JsCast};
use wasmer_wasix::{
    bin_factory::ModuleCache,
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
use crate::runtime::{TermStdout, TerminalCommandRx, WebRuntime};

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

pub const DEFAULT_BOOT_WEBC: &'static str = "sharrattj/bash";
//pub const DEFAULT_BOOT_WEBC: &str = "sharrattj/dash";
pub const DEFAULT_BOOT_USES: [&'static str; 2] = ["sharrattj/coreutils", "sharrattj/catsay"];

#[wasm_bindgen]
pub fn start() -> Result<(), JsValue> {
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
    let location = window.location().href().unwrap();

    let user_agent = USER_AGENT.clone();
    let is_mobile = wasmer_wasix::os::common::is_mobile(&user_agent);
    debug!("user_agent: {}", user_agent);

    let elem = window
        .document()
        .unwrap()
        .get_element_by_id("terminal")
        .unwrap();

    terminal.open(elem.clone().dyn_into()?);

    let (term_tx, mut term_rx) = mpsc::unbounded_channel();
    {
        let terminal: Terminal = terminal.clone().dyn_into().unwrap();
        wasm_bindgen_futures::spawn_local(async move {
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

    let runtime = Arc::new(WebRuntime::new(pool.clone(), tty_options.clone(), webgl2));
    let mut tty = Tty::new(
        Box::new(stdin_tx),
        Box::new(stdout.clone()),
        is_mobile,
        tty_options,
    );

    let compiled_modules = Arc::new(ModuleCache::new(None, None, false));

    let location = url::Url::parse(location.as_str()).unwrap();
    let mut console = if let Some(init) = location
        .query_pairs()
        .filter(|(key, _)| key == "init")
        .next()
        .map(|(_, val)| val.to_string())
    {
        let mut console = Console::new(init.as_str(), runtime.clone(), compiled_modules);
        console = console.with_no_welcome(true);
        console
    } else {
        let mut console = Console::new(DEFAULT_BOOT_WEBC, runtime.clone(), compiled_modules);
        console = console.with_uses(DEFAULT_BOOT_USES.iter().map(|a| a.to_string()).collect());
        console
    };

    let mut env = HashMap::new();
    if let Some(origin) = location.domain().clone() {
        env.insert("ORIGIN".to_string(), origin.to_string());
    }
    env.insert("LOCATION".to_string(), location.to_string());

    if let Some(prompt) = location
        .query_pairs()
        .filter(|(key, _)| key == "prompt")
        .next()
        .map(|(_, val)| val.to_string())
    {
        console = console.with_prompt(prompt);
    }

    if location
        .query_pairs()
        .any(|(key, _)| key == "no_welcome" || key == "no-welcome")
    {
        console = console.with_no_welcome(true);
    }

    if let Some(token) = location
        .query_pairs()
        .filter(|(key, _)| key == "token")
        .next()
        .map(|(_, val)| val.to_string())
    {
        console = console.with_token(token);
    }

    console = console
        .with_user_agent(user_agent.as_str())
        .with_stdin(Box::new(stdin_rx))
        .with_stdout(Box::new(stdout))
        .with_stderr(Box::new(stderr))
        .with_env(env);

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
