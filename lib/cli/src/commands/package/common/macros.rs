macro_rules! make_pb {
    ($self:ident, $msg:expr) => {{
        let pb = indicatif::ProgressBar::new_spinner();
        if $self.quiet {
            pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }

        pb.enable_steady_tick(std::time::Duration::from_millis(500));
        pb.set_style(
            indicatif::ProgressStyle::with_template("{spinner:.magenta} {msg}")
                .unwrap()
                .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷"]),
        );

        pb.set_message($msg);
        pb
    }};

    ($self:ident, $msg:expr, $($spinner:expr),+) => {{
        let pb = indicatif::ProgressBar::new_spinner();
        if $self.quiet {
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

macro_rules! pb_ok {
    ($pb:expr, $msg: expr) => {
        $pb.set_style(
            indicatif::ProgressStyle::with_template(&format!("{} {{msg}}", "✔".green().bold()))
                .unwrap(),
        );
        $pb.finish_with_message(format!("{}", $msg.bold()));
    };
}

macro_rules! pb_err {
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

macro_rules! cli_line {
    () => {
        std::env::args()
            .filter(|s| !s.starts_with("-"))
            .collect::<Vec<String>>()
            .join(" ")
    };
}

pub(crate) use bin_name;
pub(crate) use cli_line;
pub(crate) use make_pb;
pub(crate) use pb_err;
pub(crate) use pb_ok;
