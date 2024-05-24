macro_rules! make_spinner {
    ($quiet:expr, $msg:expr) => {{
        let pb = indicatif::ProgressBar::new_spinner();
        if $quiet {
            pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }

        pb.enable_steady_tick(std::time::Duration::from_millis(500));
        pb.set_style(
            indicatif::ProgressStyle::with_template("{spinner:.magenta} {msg}")
                .unwrap()
                .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷", "✶"]),
        );

        pb.set_message($msg);
        pb
    }};

    ($quiet:expr, $msg:expr, $($spinner:expr),+) => {{
        let pb = indicatif::ProgressBar::new_spinner();
        if $quiet {
            pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }

        pb.enable_steady_tick(std::time::Duration::from_millis(500));
        pb.set_style(
            indicatif::ProgressStyle::with_template("{spinner:.magenta} {msg}")
                .unwrap()
                .tick_strings(&[$($spinner),+]),
        );

        pb.set_message($msg);
        pb
    }};
}

macro_rules! spinner_ok {
    ($pb:expr, $msg: expr) => {
        $pb.set_style(
            indicatif::ProgressStyle::with_template(&format!("{} {{msg}}", "✔".green().bold()))
                .unwrap(),
        );
        $pb.finish_with_message(format!("{}", $msg.bold()));
    };
}

macro_rules! spinner_err {
    ($pb:expr, $msg: expr) => {
        $pb.set_style(
            indicatif::ProgressStyle::with_template(&format!("{} {{msg}}", "✘".red().bold()))
                .unwrap(),
        );
        $pb.finish_with_message(format!("{}", $msg.bold()));
    };
}

macro_rules! bin_name {
    () => {
        match std::env::args().nth(0) {
            Some(n) => n,
            None => String::from("wasmer"),
        }
    };
}

pub(crate) use bin_name;
pub(crate) use make_spinner;
pub(crate) use spinner_err;
pub(crate) use spinner_ok;
