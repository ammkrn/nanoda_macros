use std::collections::HashSet;
use std::fs::read_to_string;
use proc_macro2::Ident as Ident2;
use quote::format_ident;
use syn::parse_quote;

pub fn snake_case_name(ident : &syn::Ident) -> syn::Ident {
    let mut pred_upper_case : bool = false;
    let mut acc = String::new();
    let ident_string = ident.to_string();

    for c in ident_string.chars() {
        let pred_lowercase = acc.chars().last().map(|pred| pred.is_lowercase()).unwrap_or(false);
        let next_uppercase = !(c.is_lowercase());
        if pred_lowercase && next_uppercase {
            acc.push('_')
        }
        acc.push(c.to_ascii_lowercase());

    }

    syn::Ident::new(acc.as_str(), ident.span())
}


pub fn fold_with<T>(mut v : Vec<T>, t : T) -> Vec<T> {
    v.push(t);
    v
}