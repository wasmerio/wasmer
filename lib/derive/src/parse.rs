use proc_macro2::Span;
use proc_macro_error::{abort, ResultExt};
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    token, Expr, Ident, LitStr, Token,
};

pub enum WasmerAttr {
    Export {
        /// The identifier is an override, otherwise we use the field name as the name
        /// to lookup in `instance.exports`.
        identifier: Option<LitStr>,
        ty: ExportAttr,
    },
}

pub enum ExportAttr {
    NativeFunc,
    EverythingElse,
}

struct ExportExpr {
    name: Option<LitStr>,
    ty: ExportAttr,
}

struct ExportOptions {
    name: Option<LitStr>,
}
impl Parse for ExportOptions {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ident = input.parse::<Ident>()?;
        let _ = input.parse::<Token![=]>()?;
        let ident_str = ident.to_string();
        let mut name = None;

        match ident_str.as_str() {
            "name" => {
                name = Some(input.parse::<LitStr>()?);
            }
            _ => {
                // TODO: better handle errors here
                panic!("Unrecognized argument in export options");
            }
        }

        Ok(ExportOptions { name })
    }
}

// parsing either:
// Inner | NativeFunc
//
// Inner:
// - Nothing
// - `name = "name"`
impl Parse for ExportExpr {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name;
        let ty;
        /*
        // check for native_func(
        if input.peek(Ident) && input.peek2(token::Paren) {
            let ident: Ident = input.parse()?;
            let ident_str = ident.to_string();
            if ident_str.as_str() == "native_func" {
                let inner;
                let _ = parenthesized!(inner in input);
                let options = inner.parse::<ExportOptions>()?;
                name = options.name;
                ty = ExportAttr::NativeFunc;
            } else {
                panic!("Unrecognized attribute `{}` followed by `(`. Expected `native_func`",
                 &ident_str);
            }
        } 
        
        // check for inner attribute
        else */ if input.peek(Ident) {
            let options = input.parse::<ExportOptions>()?;
            name = options.name;
            ty = ExportAttr::EverythingElse;
        } else {
            name = None;
            ty = ExportAttr::EverythingElse;
        }
        Ok(Self { name, ty })
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
                let (name, ty) = if input.peek(token::Paren) {
                    let _: token::Paren = parenthesized!(export_expr in input);

                    let expr = export_expr.parse::<ExportExpr>()?;
                    (expr.name, expr.ty)
                } else {
                    (None, ExportAttr::EverythingElse)
                };

                WasmerAttr::Export {
                    identifier: name,
                    ty,
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
