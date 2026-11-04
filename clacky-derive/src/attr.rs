//! The `#[ask(..)]` attribute: a label and options.

use syn::{
    Attribute,
    Error,
    Expr,
    Ident,
    LitStr,
    Path,
    Result,
    Token,
    parse::{Parse, ParseStream},
};

pub(crate) struct Keyed<T> {
    pub(crate) key: Ident,
    pub(crate) value: T,
}

#[derive(Default)]
pub(crate) struct AskAttr {
    pub(crate) label: Option<LitStr>,
    pub(crate) hint: Option<LitStr>,
    pub(crate) placeholder: Option<Keyed<LitStr>>,
    pub(crate) default: Option<Keyed<Expr>>,
    pub(crate) validate: Option<Keyed<Path>>,
    pub(crate) optional: Option<Ident>,
    pub(crate) parse: Option<Ident>,
    pub(crate) skip: Option<Ident>,
    pub(crate) form_name: Option<Keyed<LitStr>>,
}

const OPTIONS: &str = "expected `hint`, `placeholder`, `default`, `validate`, \
                       `optional`, `parse`, `skip` or `form_name`";

fn once<T>(slot: &mut Option<T>, key: &Ident, value: T) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(Error::new(key.span(), format!("`{key}` is given twice")));
    }
    Ok(())
}

impl Parse for AskAttr {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut ask = Self::default();
        if input.peek(LitStr) {
            ask.label = Some(input.parse()?);
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "hint" => {
                    input.parse::<Token![=]>()?;
                    once(&mut ask.hint, &key, input.parse()?)?;
                }
                "placeholder" => {
                    input.parse::<Token![=]>()?;
                    let value = input.parse()?;
                    once(
                        &mut ask.placeholder,
                        &key,
                        Keyed {
                            key: key.clone(),
                            value,
                        },
                    )?;
                }
                "default" => {
                    input.parse::<Token![=]>()?;
                    let value = input.parse()?;
                    once(
                        &mut ask.default,
                        &key,
                        Keyed {
                            key: key.clone(),
                            value,
                        },
                    )?;
                }
                "validate" => {
                    input.parse::<Token![=]>()?;
                    let value = input.parse()?;
                    once(
                        &mut ask.validate,
                        &key,
                        Keyed {
                            key: key.clone(),
                            value,
                        },
                    )?;
                }
                "form_name" => {
                    input.parse::<Token![=]>()?;
                    let value = input.parse()?;
                    once(
                        &mut ask.form_name,
                        &key,
                        Keyed {
                            key: key.clone(),
                            value,
                        },
                    )?;
                }
                "optional" => once(&mut ask.optional, &key, key.clone())?,
                "parse" => once(&mut ask.parse, &key, key.clone())?,
                "skip" => once(&mut ask.skip, &key, key.clone())?,
                _ => {
                    return Err(Error::new(
                        key.span(),
                        format!("unknown `ask` option `{key}`, {OPTIONS}"),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(ask)
    }
}

impl AskAttr {
    pub(crate) fn find(attrs: &[Attribute]) -> Result<Option<Self>> {
        let mut asks = attrs.iter().filter(|attr| attr.path().is_ident("ask"));
        let Some(first) = asks.next() else {
            return Ok(None);
        };
        if let Some(second) = asks.next() {
            return Err(Error::new_spanned(second, "`#[ask]` is given twice"));
        }
        first.parse_args().map(Some)
    }

    pub(crate) fn beyond_type(&self) -> Option<proc_macro2::Span> {
        self.label
            .as_ref()
            .map(syn::spanned::Spanned::span)
            .or_else(|| self.hint.as_ref().map(syn::spanned::Spanned::span))
            .or_else(|| self.field_option().map(Ident::span))
    }

    fn field_keys(&self) -> [Option<&Ident>; 6] {
        [
            self.placeholder.as_ref().map(|option| &option.key),
            self.default.as_ref().map(|option| &option.key),
            self.validate.as_ref().map(|option| &option.key),
            self.optional.as_ref(),
            self.parse.as_ref(),
            self.skip.as_ref(),
        ]
    }

    pub(crate) fn field_option(&self) -> Option<&Ident> {
        self.field_keys().into_iter().flatten().next()
    }

    /// The first option that changes how the field is asked; `skip` does not.
    pub(crate) fn asking(&self) -> Option<&Ident> {
        let [asking @ .., _skip] = self.field_keys();
        asking.into_iter().flatten().next()
    }
}
