//! Expand translated format strings using Rust's own compile-time format checks.
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::{Parse, ParseStream}, LitStr};

struct Input { text: LitStr, arguments: proc_macro2::TokenStream }
impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self { text: input.parse()?, arguments: input.parse()? })
    }
}

#[proc_macro]
pub fn tr_format(input: TokenStream) -> TokenStream {
    let Input { text, arguments } = syn::parse_macro_input!(input as Input);
    let catalogue: std::collections::BTreeMap<String, [String; 3]> =
        serde_json::from_str(include_str!("../../i18n/catalogue.json")).expect("translation catalogue");
    let key = text.value();
    let Some(translations) = catalogue.get(&key) else {
        return syn::Error::new(text.span(), "Missing translation catalogue entry").to_compile_error().into();
    };
    // Preserve the call-site span: format! may capture local named variables.
    let en = LitStr::new(&translations[0], text.span());
    let fr = LitStr::new(&translations[1], text.span());
    let es = LitStr::new(&translations[2], text.span());
    quote! { match ::i18n::language() {
        ::i18n::Language::German => format!(#text #arguments),
        ::i18n::Language::English => format!(#en #arguments),
        ::i18n::Language::French => format!(#fr #arguments),
        ::i18n::Language::Spanish => format!(#es #arguments),
    } }.into()
}
