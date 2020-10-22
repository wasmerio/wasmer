use proc_macro2::Span;
use proc_macro_error::{abort, ResultExt};
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    token, Expr, Ident, LitStr, Token,
};

pub enum WasmerAttr {
    Export { identifier: LitStr, ty: ExportAttr },
}

pub enum ExportAttr {
    //  TODO:
    Function {},
    Memory {},
}

struct ExportExpr {
    name: LitStr,
}

impl Parse for ExportExpr {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            name: input.parse::<LitStr>()?,
        })
    }
}

// allows us to handle parens more cleanly
struct WasmerAttrInner(WasmerAttr);

impl Parse for WasmerAttrInner {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        let ident_str = ident.to_string();
        let out = match ident_str.as_str() {
            "export" => {
                let export_expr;
                let _: token::Paren = parenthesized!(export_expr in input);

                WasmerAttr::Export {
                    identifier: export_expr.parse::<ExportExpr>()?.name,
                    ty: ExportAttr::Memory {},
                }
            }
            _ => return Err(input.error(format!("Unexpected identifier {}", ident_str))),
        };
        Ok(WasmerAttrInner(out))
    }
}

impl Parse for WasmerAttr {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attr_inner;
        parenthesized!(attr_inner in input);
        Ok(attr_inner.parse::<WasmerAttrInner>()?.0)
    }
}
