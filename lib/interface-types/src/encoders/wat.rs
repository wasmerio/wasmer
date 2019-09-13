use crate::ast::{Export, Instruction, InterfaceType};

impl From<InterfaceType> for String {
    fn from(interface_type: InterfaceType) -> Self {
        match interface_type {
            InterfaceType::Int => "Int".into(),
            InterfaceType::Float => "Float".into(),
            InterfaceType::Any => "Any".into(),
            InterfaceType::String => "String".into(),
            InterfaceType::Seq => "Seq".into(),
            InterfaceType::I32 => "i32".into(),
            InterfaceType::I64 => "i64".into(),
            InterfaceType::F32 => "f32".into(),
            InterfaceType::F64 => "f64".into(),
            InterfaceType::AnyRef => "anyref".into(),
        }
    }
}

impl<'input> From<Instruction<'input>> for String {
    fn from(instruction: Instruction) -> Self {
        match instruction {
            Instruction::ArgumentGet(index) => format!("arg.get {}", index),
            Instruction::Call(index) => format!("call {}", index),
            Instruction::CallExport(export_name) => format!("call-export \"{}\"", export_name),
            Instruction::ReadUtf8 => "read-utf8".into(),
            Instruction::WriteUtf8(string) => format!("write-utf8 \"{}\"", string),
            Instruction::AsWasm(interface_type) => {
                format!("as-wasm {}", String::from(interface_type))
            }
            Instruction::AsInterface(interface_type) => {
                format!("as-interface {}", String::from(interface_type))
            }
            Instruction::TableRefAdd => "table-ref-add".into(),
            Instruction::TableRefGet => "table-ref-get".into(),
            Instruction::CallMethod(index) => format!("call-method {}", index),
            Instruction::MakeRecord(interface_type) => {
                format!("make-record {}", String::from(interface_type))
            }
            Instruction::GetField(interface_type, field_index) => {
                format!("get-field {} {}", String::from(interface_type), field_index)
            }
            Instruction::Const(interface_type, value) => {
                format!("const {} {}", String::from(interface_type), value)
            }
            Instruction::FoldSeq(import_index) => format!("fold-seq {}", import_index),
        }
    }
}

impl<'input> From<Export<'input>> for String {
    fn from(export: Export) -> Self {
        format!(
            "(@interface export \"{}\"{}{})",
            export.name,
            if export.input_types.is_empty() {
                "".into()
            } else {
                format!(
                    " (param{})",
                    export.input_types.iter().fold(
                        String::new(),
                        |mut accumulator, interface_type| {
                            accumulator.push(' ');
                            accumulator.push_str(&String::from(*interface_type));
                            accumulator
                        }
                    )
                )
            },
            if export.output_types.is_empty() {
                "".into()
            } else {
                format!(
                    " (result{})",
                    export.output_types.iter().fold(
                        String::new(),
                        |mut accumulator, interface_type| {
                            accumulator.push(' ');
                            accumulator.push_str(&String::from(*interface_type));
                            accumulator
                        }
                    )
                )
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::*;

    #[test]
    fn test_interface_types() {
        let inputs: Vec<String> = vec![
            InterfaceType::Int.into(),
            InterfaceType::Float.into(),
            InterfaceType::Any.into(),
            InterfaceType::String.into(),
            InterfaceType::Seq.into(),
            InterfaceType::I32.into(),
            InterfaceType::I64.into(),
            InterfaceType::F32.into(),
            InterfaceType::F64.into(),
            InterfaceType::AnyRef.into(),
        ];
        let outputs = vec![
            "Int", "Float", "Any", "String", "Seq", "i32", "i64", "f32", "f64", "anyref",
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_instructions() {
        let inputs: Vec<String> = vec![
            Instruction::ArgumentGet(7).into(),
            Instruction::Call(7).into(),
            Instruction::CallExport("foo").into(),
            Instruction::ReadUtf8.into(),
            Instruction::WriteUtf8("foo").into(),
            Instruction::AsWasm(InterfaceType::Int).into(),
            Instruction::AsInterface(InterfaceType::AnyRef).into(),
            Instruction::TableRefAdd.into(),
            Instruction::TableRefGet.into(),
            Instruction::CallMethod(7).into(),
            Instruction::MakeRecord(InterfaceType::Int).into(),
            Instruction::GetField(InterfaceType::Int, 7).into(),
            Instruction::Const(InterfaceType::I32, 7).into(),
            Instruction::FoldSeq(7).into(),
        ];
        let outputs = vec![
            "arg.get 7",
            "call 7",
            "call-export \"foo\"",
            "read-utf8",
            "write-utf8 \"foo\"",
            "as-wasm Int",
            "as-interface anyref",
            "table-ref-add",
            "table-ref-get",
            "call-method 7",
            "make-record Int",
            "get-field Int 7",
            "const i32 7",
            "fold-seq 7",
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_exports() {
        let inputs: Vec<String> = vec![Export {
            name: "foo",
            input_types: vec![InterfaceType::I32, InterfaceType::F32],
            output_types: vec![InterfaceType::I32],
        }
        .into()];
        let outputs = vec!["(@interface export \"foo\" (param i32 f32) (result i32))"];

        assert_eq!(inputs, outputs);
    }
}
