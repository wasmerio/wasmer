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

macro_rules! executable_instruction {
    ($name:ident ( $($argument_name:ident: $argument_type:ty),* ) -> _ $implementation:block ) => {
        use crate::interpreter::{ExecutableInstruction, wasm, stack::Stackable};

        pub(crate) fn $name<Instance, Export, LocalImport, Memory, MemoryView>(
            $($argument_name: $argument_type),*
        ) -> ExecutableInstruction<Instance, Export, LocalImport, Memory, MemoryView>
        where
            Export: wasm::structures::Export,
            LocalImport: wasm::structures::LocalImport,
            Memory: wasm::structures::Memory<MemoryView>,
            MemoryView: wasm::structures::MemoryView,
            Instance: wasm::structures::Instance<Export, LocalImport, Memory, MemoryView>,
        {
            Box::new($implementation)
        }
    };
}

#[cfg(test)]
macro_rules! test_executable_instruction {
    (
        $test_name:ident =
            instructions: [ $($instructions:expr),* $(,)* ],
            invocation_inputs: [ $($invocation_inputs:expr),* $(,)* ],
            instance: $instance:expr,
            stack: [ $($stack:expr),* $(,)* ]
            $(,)*
    ) => {
        #[test]
        #[allow(non_snake_case, unused)]
        fn $test_name() {
            use crate::interpreter::{
                instructions::tests::{Export, Instance, LocalImport, Memory, MemoryView},
                stack::Stackable,
                wasm::values::{InterfaceType, InterfaceValue},
                Instruction, Interpreter,
            };
            use std::{cell::Cell, collections::HashMap, convert::TryInto};

            let interpreter: Interpreter<Instance, Export, LocalImport, Memory, MemoryView> =
                (&vec![$($instructions),*]).try_into().unwrap();

            let invocation_inputs = vec![$($invocation_inputs),*];
            let instance = $instance;
            let run = interpreter.run(&invocation_inputs, &instance);

            assert!(run.is_ok());

            let stack = run.unwrap();

            assert_eq!(stack.as_slice(), &[$($stack),*]);
        }
    };

    (
        $test_name:ident =
            instructions: [ $($instructions:expr),* $(,)* ],
            invocation_inputs: [ $($invocation_inputs:expr),* $(,)* ],
            instance: $instance:expr,
            error: $error:expr
            $(,)*
    ) => {
        #[test]
        #[allow(non_snake_case, unused)]
        fn $test_name() {
            use crate::interpreter::{
                instructions::tests::{Export, Instance, LocalImport, Memory, MemoryView},
                stack::Stackable,
                wasm::values::{InterfaceType, InterfaceValue},
                Instruction, Interpreter,
            };
            use std::{cell::Cell, collections::HashMap, convert::TryInto};

            let interpreter: Interpreter<Instance, Export, LocalImport, Memory, MemoryView> =
                (&vec![$($instructions),*]).try_into().unwrap();

            let invocation_inputs = vec![$($invocation_inputs),*];
            let instance = $instance;
            let run = interpreter.run(&invocation_inputs, &instance);

            assert!(run.is_err());

            let error = run.unwrap_err();

            assert_eq!(error, String::from($error));
        }
    };
}
