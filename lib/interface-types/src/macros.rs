#[allow(unused)]
macro_rules! d {
    ($expression:expr) => {
        match $expression {
            tmp => {
                eprintln!(
                    "[{}:{}] {} = {:?}",
                    file!(),
                    line!(),
                    stringify!($expression),
                    &tmp
                );

                tmp
            }
        }
    };
}

macro_rules! consume {
    (($input:ident, $parser_output:ident) = $parser_expression:expr) => {
        let (next_input, $parser_output) = $parser_expression;
        $input = next_input;
    };
}
