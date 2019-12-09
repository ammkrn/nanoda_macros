use std::collections::HashSet;
use std::fs::read_to_string;
use proc_macro2::Ident as Ident2;
use quote::format_ident;
use syn::parse_quote;

#[allow(unused_variables)]
pub fn try_read_type_blacklist() -> Option<HashSet<syn::Path>> {

    let mut cwd = std::env::current_dir().ok().expect("Failed to get cwd");
    let mut cwd_sub = cwd.clone(); 
    cwd.push("type_blacklist.txt");
    cwd_sub.push("config/type_blacklist.txt");

    let mut acc = HashSet::<syn::Path>::new();

    // try to read the type blacklist in either the root directory
    // or the config sub directory. Read all uncommented lines
    // and add them to a potential user-maintained blacklist.

    for line in read_to_string(cwd).ok()
                                   .or(read_to_string(cwd_sub).ok())?
                                   .lines()
                                   .filter(|line| !(line.starts_with('#'))) {
        let line_ident = format_ident!("{}", line);
        acc.insert(parse_quote!(#line_ident));
    }

    if acc.is_empty() {
        None
    } else {
        Some(acc)
    }
}

pub fn read_trace_list() -> HashSet<Ident2> {
    let mut cwd = std::env::current_dir().ok().expect("Failed to get cwd");
    let mut cwd_sub = cwd.clone(); 
    cwd.push("traced_items.txt");
    cwd_sub.push("config/traced_items.txt");

    let mut acc = HashSet::<Ident2>::new();

    for line in read_to_string(cwd).ok()
                                   .or(read_to_string(cwd_sub).ok())
                                   .expect("Items were decorated with the `#[tracing]` macro, but no function names were specified in a `traced_items.txt` file. See crate documentation for details")
                                   .lines()
                                   .filter(|line| !(line.starts_with('#'))) {
        acc.insert(format_ident!("{}", line));
    }

    if acc.is_empty() {
        panic!("Items were decorated with the `#[tracing]` macro, but no function names were specified in a `traced_items.txt` file. See crate documentation for details");
    } else {
        acc
    }
}



// either implTrait, or 
pub fn get_collect_type(_ty : &syn::Type) -> syn::Path {
    let result = match _ty {
        syn::Type::ImplTrait(syn::TypeImplTrait { bounds, .. }) => {
            match bounds.first() {
                Some(syn::TypeParamBound::Trait(syn::TraitBound { path, .. })) => {
                    if path.segments.last().map(|x| &x.ident)  == Some(&format_ident!("Iterator")) {
                        let last_segment = path.segments.last().cloned().expect("111");
                        let seg_args = match last_segment.arguments {
                            syn::PathArguments::None => panic!("Cannot have a `None` path segment argument!"),
                            syn::PathArguments::Parenthesized(..) => panic!("not an iterator!"),
                            syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments { args, .. }) => args.clone(),
                        };

                        match seg_args.first() {
                            Some(syn::GenericArgument::Binding(syn::Binding { ty, .. })) => {
                                // return the inner path of the actual `Item = Path`
                                generic_ref_or_path(&ty)
                            },
                            _ => panic!("seg_args didn't get a generic binding!"),
                        }
                    } else {
                        panic!("Can only trace impl trait items that are Iterators! If this IS an iterator, the Iterator bound must be written first.")
                    }
                }
                _ => panic!("Cannot have empty trait boudn for arg type impl trait!")
            }
        },
        _ => generic_ref_or_path(&_ty)
    };
    result
}

// The item is either a Reference with a nested TYpe ident,
// or a normal type ident. 
pub fn generic_ref_or_path(_ty : &syn::Type) -> syn::Path {
    match _ty {
        syn::Type::Reference(syn::TypeReference { elem, .. }) => {
            match elem.as_ref() {
                syn::Type::Path(syn::TypePath { path, .. }) => {
                    path.clone()
                },
                syn::Type::Slice(syn::TypeSlice { elem, .. }) => {
                    generic_ref_or_path(&elem)
                },
                //syn::Type::Tuple(syn::TypeTuple { .. }) => {
                //},
                owise => panic!("needed type path, got {:?}\n", owise),
            }
        }
        syn::Type::Path(syn::TypePath { path, .. }) => {
            path.clone()
        }
        _ => panic!("bad type"),
    }
}


pub fn maybe_impl_iter(_ty : &syn::Type) -> Option<syn::Path> {
    match _ty {
        syn::Type::ImplTrait(syn::TypeImplTrait { bounds, .. }) => {
            match bounds.first() {
                Some(syn::TypeParamBound::Trait(syn::TraitBound { path, .. })) => {
                    if path.segments.last().map(|x| &x.ident)  == Some(&format_ident!("Iterator")) {
                        let last_segment = path.segments.last().cloned().expect("111");
                        let seg_args = match last_segment.arguments {
                            syn::PathArguments::None => panic!("Cannot have a `None` path segment argument!"),
                            syn::PathArguments::Parenthesized(..) => panic!("not an iterator!"),
                            syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments { args, .. }) => args.clone(),
                        };

                        match seg_args.first() {
                            Some(syn::GenericArgument::Binding(syn::Binding { ty, .. })) => {
                                // return the inner path of the actual `Item = Path`
                                return Some(generic_ref_or_path(ty))
                            },
                            _ => panic!("seg_args didn't get a generic binding!"),
                        }
                    } else {
                        panic!("Can only trace impl trait items that are Iterators! If this IS an iterator, the Iterator bound must be written first.")
                    }
                }
                _ => panic!("Cannot have empty trait boudn for arg type impl trait!")
            }
        },

        _ => None
    }
}



//1. Vec<&T>; let #cline_ident = #pat.clone().into_iter().cloned().collect::<Vec<T>>();
//2. impl Iterator<Item = &'e Expr>; let #clone_ident = #pat.clone().cloned().collect::<Vec<T>>();
pub fn make_local_for_arg(pat : syn::Pat, ident : Ident2, _ty : &syn::Type) -> syn::Stmt {
            // This is needed to make insert generic over owned/reference arguments
            let clone_ident = format_ident!("{}_clone__", ident);
            let item_idx_ident = format_ident!("{}_idx__", ident);

            match maybe_impl_iter(_ty) {
                Some(collect_type_path) => {
                    parse_quote! {
                        #[allow(unused_variables)]
                        let #item_idx_ident = {
                            let #clone_ident = #pat.clone().cloned().collect::<Vec<#collect_type_path>>();
                            let #item_idx_ident = trace_data__.insert_item(#clone_ident);
                            trace_data__.push_arg(this_op_idx__, #item_idx_ident);
                            #item_idx_ident
                        };
                    }
                },
                None => {
                    parse_quote! {
                        #[allow(unused_variables)]
                        let #item_idx_ident = {
                            let #clone_ident = #pat.clone();
                            let #item_idx_ident = trace_data__.insert_item(#clone_ident);
                            trace_data__.push_arg(this_op_idx__, #item_idx_ident);
                            #item_idx_ident
                        };
                    }
                }
            }
}


