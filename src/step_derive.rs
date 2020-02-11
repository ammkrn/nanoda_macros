use std::collections::{ HashSet, HashMap };
use proc_macro2::Ident as Ident2;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro::TokenStream;
use quote::{ quote, format_ident };
use syn::{ parse_macro_input, 
           parse_quote, 
           parse::Parse,
           parse::ParseStream,
           parse::ParseBuffer,
           parse::Result,
           Ident,
           visit_mut::VisitMut, 
           ItemFn, 
           punctuated::Punctuated,
           Variant,
           token::Comma,
           Field,
           Stmt };

use crate::helpers::{ fold_with, snake_case_name };

// Operates on one particular enum variant; meant to be mapped
// over the list of enum variants taken from `DeriveInput`.

// Creates an associated method for TraceMgr<T : Tracer> 
// that constructs a new step. Is slightly less type safe than we'd like
// since it's generic over items that implement HasInsertItem
pub fn gen_cnstr_one(variant_ident : &syn::Ident, unique_fields : Vec<Field>) -> syn::ItemImpl {
    // Name of constructor method, IE `EqCore enum variant uses new_eq_core`
    let method_name = format_ident!("new_{}", snake_case_name(variant_ident));

    // make the list of arguments to the constructor method (minus `&mut self`)
    let fn_args_list = unique_fields.iter().map(|field| {
        let unique_ident = field.ident.as_ref().expect("Field should have ident");
        let fn_arg_item : syn::FnArg = parse_quote! {
            #unique_ident : impl HasInsertItem
        };
        fn_arg_item
    }).collect::<Punctuated<syn::FnArg, syn::token::Comma>>();

    // Make the statemetns inserting/assigning the item indexes

    let idx_assn_stmts = unique_fields.iter().map(|field| {
        let unique_ident = &field.ident;
        let idx_ident = format_ident!("{}_idx", field.ident.as_ref().expect("Field should have ident"));
        let assn_stmt : syn::Stmt = parse_quote! {
            let #idx_ident = #unique_ident.insert_item(&mut self.item_storage);
        };
        assn_stmt
    }).collect::<Punctuated<syn::Stmt, syn::token::Semi>>();

    // Make the value fields for the return enum
    let enum_field_vals = unique_fields.iter().map(|field| {
        let unique_ident = &field.ident;
        let idx_ident = format_ident!("{}_idx", field.ident.as_ref().expect("Field should have ident"));
        let this_field_val : syn::FieldValue = parse_quote! {
            #unique_ident : #idx_ident
        };
        this_field_val
    }).collect::<Punctuated<syn::FieldValue, syn::token::Comma>>();



    //let variant_ident_w_path : syn::Path = parse_quote!("crate::trace::Step::{}", variant_ident);
    let variant_ident_w_path : syn::Path = parse_quote! {
        crate::trace::Step::#variant_ident
    };

    // Requires special handling if enum variant has no unique fields 
    // (IE only contains StepInfo)
    let mut enum_val : syn::ExprStruct = if unique_fields.is_empty() {
        parse_quote! {
            #variant_ident_w_path {
                info : StepInfo::new(self.next_safety_idx()),
            }
        }
    } else {
        parse_quote! {
            #variant_ident_w_path {
                info : StepInfo::new(self.next_safety_idx()),
                #enum_field_vals,
            }
        }
    };

    let enum_return_item = syn::Stmt::Expr(syn::Expr::Struct(enum_val));

    let mut item_impl : syn::ItemImpl = parse_quote! {
        impl<T : Tracer> TraceMgr<T> {
            pub fn #method_name(&mut self, #fn_args_list) -> Step {
                #idx_assn_stmts
                #enum_return_item
            }

        }
    };


    item_impl
}



pub fn mk_match_arm_one_short(variant_ident : &Ident, retval : &Ident) -> syn::Arm {
    let variant_ident_string = variant_ident;
    let arm : syn::Arm = parse_quote! {
        #variant_ident { .. } => stringify!(#retval)
    };
    arm
}

pub fn mk_match_arm_one(variant_ident : &syn::Ident) -> syn::Arm {
    let variant_ident_string = variant_ident;
    let arm : syn::Arm = parse_quote! {
        #variant_ident { .. } => stringify!(#variant_ident_string)
    };
    arm
}

pub fn mk_name_getters2(base_enum : &syn::ItemEnum) -> syn::ItemImpl {
    let match_arms = base_enum
                     .variants
                     .iter()
                     .map(|v| mk_match_arm_one(&v.ident))
                     .collect::<Punctuated<syn::Arm, syn::token::Comma>>();
    let item : syn::ItemImpl = parse_quote! {
        impl crate::trace::Step {
            pub fn get_step_name_string(&self) -> &'static str {
                match self {
                    #match_arms
                }
            }
        }
    };
    item
}

pub fn mk_name_getters(original_input : &syn::DeriveInput) -> syn::ItemImpl {
    let base_enum = match &original_input.data {
        syn::Data::Enum(ref syn_data_enum) => syn_data_enum,
        _ => panic!("Not an enum in mk_name_getters")
    };
    let match_arms = base_enum
                     .variants
                     .iter()
                     .map(|v| mk_match_arm_one(&v.ident))
                     .collect::<Punctuated<syn::Arm, syn::token::Comma>>();
    let item : syn::ItemImpl = parse_quote! {
        impl crate::trace::Step {
            pub fn get_step_name_string(&self) -> &'static str {
                match self {
                    #match_arms
                }
            }
        }
    };
    item
}

pub fn mk_name_getters_short(kvs : HashMap<Ident, Ident>) -> syn::ItemImpl {


    let match_arms = kvs
                     .iter()
                     .map(|(k, v)| mk_match_arm_one_short(k, v))
                     .collect::<Punctuated<syn::Arm, syn::token::Comma>>();
    let item : syn::ItemImpl = parse_quote! {
        impl crate::trace::Step {
            pub fn get_step_name_string_short(&self) -> &'static str {
                match self {
                    #match_arms
                }
            }
        }
    };
    item
}

pub fn mk_short_name_getters(original_input : &syn::DeriveInput) -> syn::ItemImpl {
    let base_enum = match &original_input.data {
        syn::Data::Enum(ref syn_data_enum) => syn_data_enum,
        _ => panic!("Not an enum in mk_name_getters")
    };
    let match_arms = base_enum
                     .variants
                     .iter()
                     .map(|v| mk_match_arm_one(&v.ident))
                     .collect::<Punctuated<syn::Arm, syn::token::Comma>>();
    let item : syn::ItemImpl = parse_quote! {
        impl crate::trace::Step {
            pub fn get_step_name_string(&self) -> &'static str {
                match self {
                    #match_arms
                }
            }
        }
    };
    item
}


// Given the derive input, 
// 1. Make sure you have a struct-style enum
// 2. collect the names of the unique fields
// 3. Make a constructor for each enum variant.
pub fn derive_cnstrs(original_input : &syn::DeriveInput) -> Vec<syn::ItemImpl> {


    match &original_input.data {
        syn::Data::Enum(syn::DataEnum { variants, .. }) => {


            let unique_fields_cumul = get_unique_fields(&variants);

            // This is the cumulative set of all unique fields
            // In this case, anything other than "info".

            variants.iter().map(|v| {
                let v_ident = v.ident.clone();
                let this_variant_unique_fields = match v.fields {
                    syn::Fields::Named(ref fields_named) => {
                        fields_named.named.iter().filter(|f| unique_fields_cumul.contains(f))
                        .cloned()
                        .collect::<Vec<Field>>()
                    },
                    _ => panic!("Not named fields as required")
                };
                gen_cnstr_one(&v_ident, this_variant_unique_fields)
            }).collect::<Vec<syn::ItemImpl>>()



        },
        syn::Data::Struct(..) => panic!("StepMk :: StepDerive requires an enum input, got a struct!"),
        syn::Data::Union(..) => panic!("StepMk :: StepDerive requires an enum input, got a union!")
    }
}

pub fn derive_cnstrs2(base_enum : &syn::ItemEnum) -> Vec<syn::ItemImpl> {

    let unique_fields_cumul = get_unique_fields(&base_enum.variants);

    // This is the cumulative set of all unique fields
    // In this case, anything other than "info".
    base_enum.variants.iter().map(|v| {
        let v_ident = v.ident.clone();
        let this_variant_unique_fields = match v.fields {
            syn::Fields::Named(ref fields_named) => {
                fields_named.named.iter().filter(|f| unique_fields_cumul.contains(f))
                .cloned()
                .collect::<Vec<Field>>()
            },
            _ => panic!("Not named fields as required")
        };
        gen_cnstr_one(&v_ident, this_variant_unique_fields)
    }).collect::<Vec<syn::ItemImpl>>()
}

pub fn get_unique_fields(variants : &Punctuated<Variant, Comma>) -> HashSet<Field> {
    fields_diff(variants)
}

pub fn fields_inter(variants : &Punctuated<Variant, Comma>) -> HashSet<Field> {
    let mut named_fields = 
        variants
        .iter()
        .filter_map(|v| match &v.fields {
            syn::Fields::Named(fields_named) => Some(fields_named.named.clone().into_iter().collect::<HashSet<Field>>()),
            _ => None
        }).collect::<Vec<HashSet<Field>>>();

    let mut acc_set = named_fields.pop().unwrap_or_else(|| HashSet::new());

    for s in named_fields.iter() {
        let intersection = acc_set.intersection(s).cloned().collect::<HashSet<Field>>();
        acc_set = intersection
    }

    acc_set
}

pub fn fields_union(variants : &Punctuated<Variant, Comma>) -> HashSet<Field> {
    let mut union = HashSet::new();
    for elem in variants
               .iter()
               .filter_map(|v| match &v.fields {
                   syn::Fields::Named(fields_named) => Some(fields_named.named.clone()),
                   _ => None
               }).flat_map(|named| named.into_iter()) {
                   union.insert(elem);
               }
    union
}

pub fn fields_diff(variants : &Punctuated<Variant, Comma>) -> HashSet<Field> {
    let union = fields_union(variants);
    let inter = fields_inter(variants);

    union.difference(&inter).cloned().collect::<HashSet<Field>>()
}



// Result HashMap has the complete set of variants even if not all
// have #[short(..)] attributes; for those without, it just uses
// the long name.
pub fn collect_short_attrs(base_enum : &mut syn::ItemEnum) -> HashMap<Ident, Ident> {
    let mut acc = HashMap::<Ident, Ident>::new();

    let desired_path : syn::Path = parse_quote!(short);

    for variant in base_enum
                  .variants
                  .iter_mut() {
        let variant_ident = variant.ident.clone();
        let taken_attrs = std::mem::replace(&mut variant.attrs, Vec::new());
        if let Some(short_attr) = taken_attrs
                                  .iter()
                                  .find(|attr| &attr.path == &desired_path) {
            let parsed_attr = short_attr.parse_meta().expect("`short` attr was in the wrong format; could not be parsed as inert attribute");

            match parsed_attr {
                syn::Meta::List(syn::MetaList { nested, .. }) => {
                    match nested.first().expect("Failed to get first element of `nested` when parsing short attr") {
                        syn::NestedMeta::Meta(syn::Meta::Path(syn::Path { segments, .. })) => {
                            let taken_ident = segments.first().expect("Failed to get ident from leading path segment in short attr").ident.clone();
                            acc.insert(variant_ident, taken_ident);
                        },
                        _ => panic!("Attribute list for `short` was empty; needs to have the desired name in parenthesis!")
                    }
                },
                _ => panic!("`short` attribute must be followed by the desired short name in parenthesis, ie `#[short(AQ)]")
            }

        } else {
            acc.insert(variant_ident.clone(), variant_ident);
        }
    }

    let mut nodup_set = HashSet::<&Ident>::new();
    for (k, v) in acc.iter() {
        if !nodup_set.insert(v) {
            panic!("Duplicate short names found : identifier {} is used more than once!", v)
        }
    }


    acc
}
