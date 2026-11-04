//! `#[derive(Choices)]` and `#[derive(Form)]` for clacky: a unit-only enum
//! becomes a choice, a struct with named fields becomes a form asked field by
//! field.
#![deny(missing_docs)]

mod attr;
mod words;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    Data,
    DataEnum,
    DataStruct,
    DeriveInput,
    Error,
    Fields,
    FieldsNamed,
    GenericArgument,
    Ident,
    LitStr,
    PathArguments,
    Result,
    Type,
    ext::IdentExt,
    parse_macro_input,
    spanned::Spanned,
};

use crate::{attr::AskAttr, words::words};

/// Makes an enum of unit variants a choice (the dependency must be named
/// `clacky`), labelled by its name in words unless `#[ask("Label")]`, needing
/// no `PartialEq` (two variants with one label are a compile error);
/// `#[ask("Label", hint = "..")]` on a variant sets its label and hint.
///
/// ```ignore
/// #[derive(clacky::Choices)]
/// enum Kind {
///     #[ask("Library")]
///     Lib,
///     #[ask("Binary", hint = "recommended")]
///     Bin,
///     HttpServer,
/// }
/// ```
#[proc_macro_derive(Choices, attributes(ask))]
pub fn derive_choices(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    choices(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// Makes a struct with named fields a form asked field by field, with a setter
/// per field (`with_` first when named like an `Ask` method) taking an
/// `Option`, and `#[ask(form_name = "..")]` renaming `TypeForm`; a field takes
/// `#[ask("Label", default = .., placeholder = "..", validate = path, optional,
/// parse, skip)]`.
///
/// ```ignore
/// #[derive(clacky::Form)]
/// struct Setup {
///     #[ask("Name", placeholder = "my-app")]
///     name: String,
///     #[ask("Port", default = 8080)]
///     port: u16,
/// }
///
/// let setup = session.ask(Setup::form())?;
/// ```
#[proc_macro_derive(Form, attributes(ask))]
pub fn derive_form(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    form(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn refuse_generics(input: &DeriveInput, derive: &str) -> Result<()> {
    input.generics.params.first().map_or(Ok(()), |param| {
        Err(Error::new_spanned(
            param,
            format!("`#[derive({derive})]` does not support generics"),
        ))
    })
}

fn choices(input: &DeriveInput) -> Result<Tokens> {
    if let Some(attr) = input.attrs.iter().find(|attr| attr.path().is_ident("ask")) {
        return Err(Error::new_spanned(
            attr,
            "an enum takes no `#[ask]` of its own; labels and hints go on its variants",
        ));
    }
    refuse_generics(input, "Choices")?;
    match &input.data {
        Data::Enum(data) => expand_choices(&input.ident, data),
        Data::Struct(data) => Err(Error::new(
            data.struct_token.span(),
            "`#[derive(Choices)]` is for enums; use #[derive(Form)] for a struct",
        )),
        Data::Union(data) => Err(Error::new(
            data.union_token.span(),
            "`#[derive(Choices)]` supports enums of unit variants",
        )),
    }
}

fn form(input: &DeriveInput) -> Result<Tokens> {
    let on_type = AskAttr::find(&input.attrs)?.unwrap_or_default();
    if let Some(span) = on_type.beyond_type() {
        return Err(Error::new(
            span,
            "only `form_name` goes on the type; the rest goes on fields",
        ));
    }
    refuse_generics(input, "Form")?;
    match &input.data {
        Data::Struct(data) => expand_form(input, data, &on_type),
        Data::Enum(data) => Err(Error::new(
            data.enum_token.span(),
            "`#[derive(Form)]` is for structs with named fields; use \
             #[derive(Choices)] for an enum",
        )),
        Data::Union(data) => Err(Error::new(
            data.union_token.span(),
            "`#[derive(Form)]` supports structs with named fields",
        )),
    }
}

fn refuse_form_name(ask: &AskAttr, place: &str) -> Result<()> {
    ask.form_name.as_ref().map_or(Ok(()), |option| {
        Err(Error::new(
            option.key.span(),
            format!("`form_name` goes on the struct, not on {place}"),
        ))
    })
}

fn expand_choices(name: &Ident, data: &DataEnum) -> Result<Tokens> {
    if data.variants.is_empty() {
        return Err(Error::new_spanned(
            name,
            "an enum without variants has nothing to choose",
        ));
    }
    let mut choices = Vec::new();
    let mut labels = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                &variant.fields,
                "`#[derive(Choices)]` supports only unit variants",
            ));
        }
        let ask = AskAttr::find(&variant.attrs)?.unwrap_or_default();
        refuse_form_name(&ask, "variants")?;
        if let Some(option) = ask.field_option() {
            return Err(Error::new(
                option.span(),
                format!("`{option}` goes on struct fields, not on variants"),
            ));
        }
        let (label, hint) = (ask.label, ask.hint);
        let at = label
            .as_ref()
            .map_or_else(|| variant.ident.span(), Spanned::span);
        let label = label.map_or_else(
            || words(&variant.ident.unraw().to_string()),
            |label| label.value(),
        );
        if !seen.insert(label.clone()) {
            return Err(Error::new(
                at,
                format!(
                    "two variants are labelled \"{label}\"; a given answer would \
                     find only the first, give one of them another `#[ask(\"..\")]` label"
                ),
            ));
        }
        let ident = &variant.ident;
        let hint = hint.map(|hint| quote!(.hint(#hint)));
        choices.push(quote! {
            ::clacky::Choice::new(#name::#ident, #label) #hint
        });
        labels.push(quote!(#name::#ident => #label));
    }
    Ok(quote! {
        impl ::clacky::core::askable::Choices for #name {
            fn choices() -> ::std::vec::Vec<::clacky::Choice<Self>> {
                ::std::vec![#(#choices),*]
            }

            fn label(&self) -> &'static str {
                match self {
                    #(#labels,)*
                }
            }
        }

        impl ::clacky::core::askable::Answer for #name {
            type Prompt = ::clacky::prompts::select::Select<
                Self,
                ::clacky::prompts::choice::ByLabel<Self>,
            >;

            fn prompt(message: &str) -> Self::Prompt {
                <Self as ::clacky::core::askable::Choices>::select(message)
            }
        }
    })
}

struct Sorted<'a> {
    asked: Vec<&'a Ident>,
    types: Vec<&'a Type>,
    prompts: Vec<Tokens>,
    skipped: Vec<(&'a Ident, &'a Type)>,
}

fn read_fields(named: &FieldsNamed) -> Result<Sorted<'_>> {
    let mut asked = Vec::new();
    let mut types = Vec::new();
    let mut prompts = Vec::new();
    let mut skipped = Vec::new();
    for field in &named.named {
        let Some(ident) = &field.ident else {
            continue;
        };
        let ask = AskAttr::find(&field.attrs)?.unwrap_or_default();
        refuse_form_name(&ask, "fields")?;
        if let Some(hint) = &ask.hint {
            return Err(Error::new_spanned(
                hint,
                "a field has a label only; `hint` goes on enum variants",
            ));
        }
        if ask.skip.is_some() {
            let other = ask
                .label
                .as_ref()
                .map(Spanned::span)
                .or_else(|| ask.asking().map(Ident::span));
            if let Some(other) = other {
                return Err(Error::new(
                    other,
                    "a skipped field is not asked, so it takes no other option",
                ));
            }
            skipped.push((ident, &field.ty));
            continue;
        }
        let label = ask
            .label
            .as_ref()
            .map_or_else(|| words(&ident.unraw().to_string()), LitStr::value);
        prompts.push(field_prompt(&field.ty, &label, &ask)?);
        asked.push(ident);
        types.push(&field.ty);
    }
    if asked.is_empty() {
        return Err(Error::new_spanned(
            named,
            "a struct whose fields are all skipped asks nothing",
        ));
    }
    Ok(Sorted {
        asked,
        types,
        prompts,
        skipped,
    })
}

/// The methods of `Ask`: a setter named like one would hide it on the form.
const ASK_METHODS: [&str; 4] = ["run", "map", "abort", "timeout"];

/// Setter name: `with_` first when the field is named like an `Ask` method.
fn setter_name(field: &Ident) -> Ident {
    let name = field.unraw().to_string();
    if ASK_METHODS.contains(&name.as_str()) {
        format_ident!("with_{name}")
    } else {
        field.clone()
    }
}

fn expand_form(
    input: &DeriveInput,
    data: &DataStruct,
    on_type: &AskAttr,
) -> Result<Tokens> {
    let Fields::Named(named) = &data.fields else {
        return Err(Error::new_spanned(
            &data.fields,
            "`#[derive(Form)]` supports only structs with named fields",
        ));
    };
    if named.named.is_empty() {
        return Err(Error::new_spanned(
            named,
            "a struct without fields asks nothing",
        ));
    }
    let name = &input.ident;
    let vis = &input.vis;
    let Sorted {
        asked,
        types,
        prompts,
        skipped,
    } = read_fields(named)?;
    let form = form_ident(input, on_type)?;
    let form_doc = format!(
        "The questions of [`{name}`], with the answers already known: a field \
         set to `Some` is taken and shown answered, one left `None` is asked."
    );
    let setters = asked.iter().zip(&types).map(|(field, ty)| {
        let setter = setter_name(field);
        let doc = format!(
            "Takes `{}` as the answer when `Some`, asks it when `None`.",
            field.unraw()
        );
        quote! {
            #[doc = #doc]
            #[must_use]
            #vis fn #setter(mut self, #field: ::std::option::Option<#ty>) -> Self {
                self.#field = #field;
                self
            }
        }
    });
    let skipped = skipped.iter().map(|(field, ty)| {
        quote_spanned! {ty.span()=> #field: ::std::default::Default::default()}
    });
    let shown = asked.iter().map(|field| field.unraw().to_string());
    let form_text = form.to_string();
    Ok(quote! {
        #[doc = #form_doc]
        #vis struct #form {
            #(#asked: ::std::option::Option<#types>,)*
        }

        impl ::std::fmt::Debug for #form {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(#form_text)
                    #(.field(#shown, &self.#asked.is_some()))*
                    .finish()
            }
        }

        impl #form {
            #(#setters)*
        }

        impl #name {
            /// Asks each field in order; see the setters of the form to give answers already known.
            #[must_use]
            #vis fn form() -> #form {
                #form {
                    #(#asked: ::std::option::Option::None,)*
                }
            }
        }

        impl ::clacky::core::ask::Ask for #form {
            type Output = #name;

            fn run<__ClackyTerminal: ::clacky::terminal::Terminal>(
                self,
                session: &::clacky::core::session::Session<__ClackyTerminal>,
            ) -> ::clacky::core::error::Result<#name> {
                ::std::result::Result::Ok(#name {
                    #(#asked: ::clacky::core::ask::Ask::run(
                        ::clacky::core::askable::Giving::give(#prompts, self.#asked),
                        session,
                    )?,)*
                    #(#skipped,)*
                })
            }
        }
    })
}

fn form_ident(input: &DeriveInput, on_type: &AskAttr) -> Result<Ident> {
    on_type.form_name.as_ref().map_or_else(
        || Ok(format_ident!("{}Form", input.ident)),
        |name| name.value.parse::<Ident>(),
    )
}

fn shape(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| {
        if let GenericArgument::Type(inner) = arg {
            Some(inner)
        } else {
            None
        }
    })
}

fn none() -> Tokens {
    quote!(::std::option::Option::None)
}

fn field_prompt(ty: &Type, label: &str, ask: &AskAttr) -> Result<Tokens> {
    let shape = shape(ty).unwrap_or_default();
    let is = |names: &[&str]| names.contains(&shape.as_str());
    if let Some(placeholder) = &ask.placeholder {
        if is(&["bool", "Vec", "Date"]) {
            return Err(Error::new(
                placeholder.key.span(),
                format!("`placeholder` is for text, and `{shape}` is not typed in"),
            ));
        }
    }
    if let Some(default) = &ask.default {
        if is(&["Vec", "Option"]) {
            return Err(Error::new(
                default.key.span(),
                format!("`default` does not go on a `{shape}` field"),
            ));
        }
    }
    if let Some(optional) = &ask.optional {
        if !is(&["Vec"]) {
            return Err(Error::new(
                optional.span(),
                "`optional` is for `Vec` fields; an answer that may be left empty \
                 is an `Option<T>`",
            ));
        }
    }
    if let Some(parse) = &ask.parse {
        if is(&["bool", "Vec", "Date"]) {
            return Err(Error::new(
                parse.span(),
                format!("`parse` reads text, and `{shape}` is not typed in"),
            ));
        }
    }
    if let Some(validate) = &ask.validate {
        if is(&["Option"]) {
            return Err(Error::new(
                validate.key.span(),
                "`validate` does not go on an `Option` field yet",
            ));
        }
    }
    let span = ty.span();
    let placeholder = ask.placeholder.as_ref().map(|option| &option.value);
    let default = ask.default.as_ref().map(|option| &option.value);
    let mut prompt = if ask.parse.is_some() {
        let mut text = quote!(::clacky::prompts::text::text(#label));
        if let Some(placeholder) = placeholder {
            text = quote!(#text.placeholder(#placeholder));
        }
        if let Some(default) = default {
            text =
                quote!(#text.default(::std::string::ToString::to_string(&(#default))));
        }
        option_inner(ty).map_or_else(
            || {
                quote_spanned! {span=>
                    ::clacky::core::ask::Peek::parse::<#ty>(#text)
                }
            },
            |inner| {
                quote_spanned! {span=>
                    ::clacky::core::askable::OrBlank::new(
                        ::clacky::core::ask::Peek::parse::<
                            ::clacky::core::askable::Blank<#inner>
                        >(#text.default("")),
                    )
                }
            },
        )
    } else if placeholder.is_some() || default.is_some() {
        let placeholder = placeholder
            .map_or_else(none, |it| quote!(::std::option::Option::Some(#it)));
        let default =
            default.map_or_else(none, |it| quote!(::std::option::Option::Some(#it)));
        quote_spanned! {span=>
            <#ty as ::clacky::core::askable::Fillable>::filled(#label, #placeholder, #default)
        }
    } else {
        quote_spanned! {span=>
            <#ty as ::clacky::core::askable::Answer>::prompt(#label)
        }
    };
    if ask.optional.is_some() {
        prompt = quote!(#prompt.picked(::std::iter::empty()).optional());
    }
    if let Some(validate) = &ask.validate {
        let check = &validate.value;
        prompt = quote!(::clacky::core::ask::Peek::validate(#prompt, #check));
    }
    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use syn::{DeriveInput, parse_quote};

    use super::{choices, form};

    fn refused_choices(input: &DeriveInput) -> String {
        choices(input)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }

    fn refused(input: &DeriveInput) -> String {
        form(input)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn a_non_unit_variant_is_refused() {
        let error = refused_choices(&parse_quote! { enum Kind { Lib(u8) } });
        assert_eq!(error, "`#[derive(Choices)]` supports only unit variants");
    }

    #[test]
    fn two_variants_with_one_label_are_refused() {
        let named = refused_choices(
            &parse_quote! { enum Kind { #[ask("Lib")] A, #[ask("Lib")] B } },
        );
        assert!(
            named.contains("two variants are labelled \"Lib\""),
            "{named}"
        );
        let by_name =
            refused_choices(&parse_quote! { enum Kind { Lib, #[ask("Lib")] B } });
        assert!(
            by_name.contains("two variants are labelled \"Lib\""),
            "{by_name}"
        );
        let fine = refused_choices(&parse_quote! { enum Kind { Lib, Bin } });
        assert_eq!(fine, "");
    }

    #[test]
    fn a_tuple_struct_is_refused() {
        let error = refused(&parse_quote! { struct Setup(Kind); });
        assert_eq!(
            error,
            "`#[derive(Form)]` supports only structs with named fields"
        );
    }

    #[test]
    fn each_derive_refuses_the_others_shape() {
        let on_struct = refused_choices(&parse_quote! { struct Setup { kind: Kind } });
        assert!(on_struct.contains("use #[derive(Form)]"), "{on_struct}");
        let on_enum = refused(&parse_quote! { enum Kind { Lib } });
        assert!(on_enum.contains("use #[derive(Choices)]"), "{on_enum}");
    }

    #[test]
    fn an_unknown_option_is_refused() {
        let error = refused_choices(
            &parse_quote! { enum Kind { #[ask("Lib", color = "red")] Lib } },
        );
        assert_eq!(
            error,
            "unknown `ask` option `color`, expected `hint`, `placeholder`, \
             `default`, `validate`, `optional`, `parse`, `skip` or `form_name`"
        );
    }

    #[test]
    fn options_in_the_wrong_place_are_refused() {
        let on_variant =
            refused_choices(&parse_quote! { enum Kind { #[ask(skip)] Lib } });
        assert_eq!(on_variant, "`skip` goes on struct fields, not on variants");
        let placeholder =
            refused(&parse_quote! { struct S { #[ask(placeholder = "x")] on: bool } });
        assert!(
            placeholder.contains("`placeholder` is for text"),
            "{placeholder}"
        );
        let default =
            refused(&parse_quote! { struct S { #[ask(default = 1)] v: Vec<K> } });
        assert!(
            default.contains("`default` does not go on a `Vec`"),
            "{default}"
        );
        let optional =
            refused(&parse_quote! { struct S { #[ask(optional)] n: String } });
        assert!(
            optional.contains("`optional` is for `Vec` fields"),
            "{optional}"
        );
        let parse = refused(&parse_quote! { struct S { #[ask(parse)] on: bool } });
        assert!(parse.contains("`parse` reads text"), "{parse}");
        let skipped = refused(&parse_quote! { struct S { #[ask("A", skip)] a: u8 } });
        assert!(
            skipped.contains("a skipped field is not asked"),
            "{skipped}"
        );
        let all = refused(&parse_quote! { struct S { #[ask(skip)] a: u8 } });
        assert!(all.contains("all skipped"), "{all}");
        let twice = refused(&parse_quote! { struct S { #[ask(skip, skip)] a: u8 } });
        assert_eq!(twice, "`skip` is given twice");
    }

    #[test]
    fn a_hint_on_a_field_is_refused() {
        let error = refused(
            &parse_quote! { struct Setup { #[ask("Kind", hint = "x")] kind: Kind } },
        );
        assert!(error.contains("`hint` goes on enum variants"), "{error}");
    }

    #[test]
    fn the_attribute_is_refused_on_the_type_and_given_once() {
        let on_enum = refused_choices(&parse_quote! { #[ask("X")] enum Kind { Lib } });
        assert!(
            on_enum.contains("takes no `#[ask]` of its own"),
            "{on_enum}"
        );
        let on_struct = refused(&parse_quote! { #[ask("X")] struct S { a: u8 } });
        assert!(
            on_struct.starts_with("only `form_name` goes on the type"),
            "{on_struct}"
        );
        let twice = refused_choices(
            &parse_quote! { enum Kind { #[ask("A")] #[ask("B")] Lib } },
        );
        assert!(twice.contains("given twice"), "{twice}");
    }

    #[test]
    fn generics_unions_and_empty_types_are_refused() {
        assert!(
            refused_choices(&parse_quote! { enum Kind<T> { Lib(T) } })
                .contains("generics")
        );
        assert!(refused_choices(&parse_quote! { union U { a: u8 } }).contains("enums"));
        assert!(refused(&parse_quote! { union U { a: u8 } }).contains("structs"));
        assert!(
            refused_choices(&parse_quote! { enum Kind {} })
                .contains("nothing to choose")
        );
        assert!(refused(&parse_quote! { struct S {} }).contains("asks nothing"));
    }

    #[test]
    fn a_unit_enum_and_a_named_struct_expand() {
        assert!(
            choices(&parse_quote! { enum Kind { Lib, #[ask("Bin", hint = "x")] Bin } })
                .is_ok()
        );
        assert!(form(&parse_quote! { pub struct Setup { kind: Kind } }).is_ok());
        assert!(
            form(&parse_quote! {
                struct Setup {
                    #[ask("Port", default = 8080, validate = free)]
                    port: u16,
                    #[ask(placeholder = "x", parse)]
                    id: Option<Id>,
                    #[ask(optional)]
                    tools: Vec<Tool>,
                    #[ask(skip)]
                    seen: bool,
                }
            })
            .is_ok()
        );
    }

    #[test]
    fn form_name_goes_on_a_struct_only() {
        let error = refused(&parse_quote! {
            struct Setup { #[ask(form_name = "X")] name: String }
        });
        assert_eq!(error, "`form_name` goes on the struct, not on fields");
        let on_variant = refused_choices(&parse_quote! {
            enum Kind { #[ask(form_name = "X")] Lib }
        });
        assert_eq!(
            on_variant,
            "`form_name` goes on the struct, not on variants"
        );
    }

    #[test]
    fn validate_does_not_go_on_an_option_field() {
        let error = refused(&parse_quote! {
            struct S { #[ask(validate = check)] nick: Option<String> }
        });
        assert!(
            error.contains("`validate` does not go on an `Option`"),
            "{error}"
        );
    }

    #[test]
    fn a_setter_that_would_hide_an_ask_method_gets_with_first() {
        let source = form(&parse_quote! {
            struct Job { run: bool, map: u8, abort: bool, timeout: u8, port: u16 }
        })
        .map(|tokens| tokens.to_string())
        .unwrap_or_default();
        assert!(source.contains("fn with_run"), "{source}");
        assert!(source.contains("fn with_map"), "{source}");
        assert!(source.contains("fn with_abort"), "{source}");
        assert!(source.contains("fn with_timeout"), "{source}");
        assert!(source.contains("fn port"), "{source}");
    }
}
