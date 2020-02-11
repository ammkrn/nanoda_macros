#![allow(unused_parens)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unreachable_code)]
extern crate proc_macro;
use proc_macro::TokenStream;

use std::collections::HashSet;
use proc_macro2::Ident as Ident2;
use quote::{ quote, format_ident };
use syn::{ parse_macro_input, 
           parse_quote, 
           parse::Parse,
           parse::ParseStream,
           parse::Result,
           visit_mut::VisitMut, 
           ItemFn, 
           punctuated::Punctuated,
           Stmt };

mod helpers;
mod step_derive;

// The acceptable forms of `Step` type annotation.
fn type_is_step(type_ : &syn::Type) -> bool {
    let form1 : syn::Type = parse_quote!(Step);
    let form2 : syn::Type = parse_quote!(trace::Step);
    let form3 : syn::Type = parse_quote!(crate::trace::Step);

       (type_ == &form1)
    || (type_ == &form2)
    || (type_ == &form3)

}

/*
Safety check; ensure that either: 
1. the `let this_step ...` local binding already has a type annotation of ` : Step `
2. A type annotation of ` : Step` is added if it's not already there.
*/
fn step_declar_add_type(_stmt : &syn::Stmt) -> (syn::Ident, syn::Stmt) {
    let full_step_type : syn::Type = parse_quote! {
        crate::trace::Step
    };

    match _stmt {
        syn::Stmt::Local(local) => {
            let mut init_copy = local.init.as_ref().clone();

            match &local.pat {
                syn::Pat::Ident(syn::PatIdent { ident, .. }) => {
                    let var_name = ident.clone();
                    let init_val = local.init.clone();
                    let mut new_stmt : syn::Stmt = parse_quote! {
                        let #var_name : #full_step_type = panic!();
                    };

                    match new_stmt {
                        syn::Stmt::Local(ref mut new_local) => {
                            std::mem::replace(&mut new_local.attrs, local.attrs.clone());
                            std::mem::replace(&mut new_local.init, local.init.clone());
                            (var_name, new_stmt)

                        },
                        _ => panic!("nanoda_macros::step_declar_add_type, line {} : the variable `new_stmt` \
                                     was not parsed as a syn::Stmt::Local. While not a big deal, this is \
                                     probably not a user-serviceable
                                     error, please report this to the author", line!())
                    }
                },
                syn::Pat::Type(pat_type) => {
                    if type_is_step(pat_type.ty.as_ref()) {
                        let local_ident = match pat_type.pat.as_ref() {
                            syn::Pat::Ident(pat_ident) => {
                                pat_ident.ident.clone()
                            },
                            _ => panic!("Not a pat::Ident")
                        };
                        (local_ident, _stmt.clone())
                    } else {
                        panic!("tracing macro expected the first statement of the block to be a\
                               local whose type is `Step`, but its type was {:#?}\n", pat_type.ty)
                    }
                },
                _ => panic!("tracing macro expected the first statement of the block to be a\
                              local of the form `let s : Step = mk_step()...` or `let s = mk_step()`")
            }
        },
        _ => panic!("tracing macro expected the first statement of the block to be a\
                     local (IE `let x : usize = 0;`, but instead it got some other type of statement!")
    }
}


fn is_local_stmt(s : &Stmt) -> bool {
    match s {
        syn::Stmt::Local(..) => true,
        _ => false
    }
}

fn change_call_ident(e : &mut syn::Expr) {
    match e {
        syn::Expr::Call(syn::ExprCall { func, .. }) => {
            match func.as_mut() {
                syn::Expr::Path(syn::ExprPath { path, .. }) => {
                    let mut last_mut = path.segments.last_mut().expect("Failed to get last path segment in change_call_ident");
                    let snakecase = crate::helpers::snake_case_name(&last_mut.ident);
                    let new_snakecase = format_ident!("new_{}", snakecase);
                    std::mem::replace(&mut last_mut.ident, new_snakecase);
                },
                _ => panic!("Expected Path in Expr::Call in change_call_ident")
            }
        },
        _ => panic!("Expected Expr::Call in change_call_ident")
    }
}

// Only needs to be mut so at the end we can swap the old
// x.block.stmts with the new block stmts vec.
fn add_tracing_to_item_fn(mut trace_attr : TraceAttr, mut item_fn : syn::ItemFn) -> syn::ItemFn {

    let snake_cased = change_call_ident(&mut trace_attr.step);

    let mut closure_block : syn::Block = item_fn.block.as_ref().clone();
    trace_attr.visit_block_mut(&mut closure_block);

    let this_step_cnstr = &trace_attr.step;
    let trace_mgr_loc = &trace_attr.tracer_location;

    let return_type : syn::Type = match (&item_fn.sig.output) {
        syn::ReturnType::Default => parse_quote! { () },
        syn::ReturnType::Type(_, boxed_type) => boxed_type.as_ref().clone()
    };

    let step_declar = parse_quote! { let this_step : crate::trace::Step = (#trace_mgr_loc).write().#this_step_cnstr; };

    let rest_as_closure : syn::ExprClosure = parse_quote!(|| #closure_block);

    // instead of `#trace_mgr_loc`
    // use what you parsed from the `attr` field,
    // which tells you where you can find the mutable reference to trace_mgr.
    let mut new_block_stmts : Vec::<syn::Stmt> = vec![
        // Before closure/function body
        parse_quote! { use crate::trace::HasInsertItem; },
        parse_quote! { use crate::trace::Tracer; },

        step_declar,

        parse_quote! { let stack_size_before = (#trace_mgr_loc).read().stack_len() ; },
        parse_quote! { let ___safety_idx_before = *(this_step.get_safety_idx()); },
        parse_quote! { (#trace_mgr_loc).write().push(this_step); },

        // Closure + closure.call()
        parse_quote! { let result____ : #return_type = { #rest_as_closure }(); },
        
        // After closure :
        parse_quote! { let mut write_guard = (#trace_mgr_loc).write(); },

        parse_quote! { let result_idx = result____.clone().insert_item(&mut (*write_guard).item_storage); },
        parse_quote! { let mut recovered_this_step = write_guard.pop(); },
        parse_quote! { let this_step_idx = write_guard.next_step_idx(); },

        // Assert that current step's `self_idx` was uninitialized/None
        // Then replace with the generated index.
        parse_quote! { assert!(recovered_this_step.get_self_idx().is_none()); },
        parse_quote! { std::mem::replace(recovered_this_step.get_mut_self_idx(), Some(this_step_idx)); },

        // Add this steps' index to it's parent's list of child steps
        parse_quote! { write_guard.add_child(this_step_idx, &recovered_this_step); },

        // Asser that the current step's result was uninitialized/None
        // then initialize it.
        parse_quote! { assert!(recovered_this_step.get_result().is_none()); },
        parse_quote! { std::mem::replace(recovered_this_step.get_mut_result(), Some(result_idx)); },

        // Execute the trace() function on this step before dropping it.
        parse_quote! { write_guard.trace_step(&recovered_this_step); },

        // Assert sanity check invariants
        parse_quote! { assert_eq!(stack_size_before, write_guard.stack_len()); },
        parse_quote! { assert_eq!(___safety_idx_before, *(recovered_this_step.get_safety_idx())); },
   ];

    // push final return statement; parse_quote doesn't want to do this as
    // a `syn::Stmt::Expr`; complains about no semicolon.
    new_block_stmts.push(syn::Stmt::Expr(parse_quote! { result____ }));

    std::mem::replace(&mut item_fn.block.stmts, new_block_stmts);
    item_fn
}





struct TraceAttr {
    pub tracer_location : syn::Expr,
    pub step : syn::Expr,
}

impl TraceAttr {
    pub fn new(tracer_location : syn::Expr, step : syn::Expr) -> Self {
        TraceAttr {
            tracer_location,
            step
        }
    }
    
}

impl VisitMut for TraceAttr {
    fn visit_expr_method_call_mut(&mut self, x : &mut syn::ExprMethodCall) {
        let target_ident = format_ident!("push_extra");

        let new_arg : syn::Expr = parse_quote!(___safety_idx_before);

        if &x.method == &target_ident {
            x.args.push(new_arg)
        }
    }
}

// Expr::Field, Expr::Call
// or
// Expr::Reference, Expr::Call
impl Parse for TraceAttr {
    fn parse(input : ParseStream) -> Result<TraceAttr> {
        use syn::punctuated::Punctuated;
        use syn::token::Comma;

        let mut parsed = match Punctuated::<syn::Expr, Comma>::parse_terminated(input) {
            Ok(p) => p.into_iter(),
            Err(e) => panic!("Failed to parse Trace Attribute as #[trace(trace_loc, step)]. Error : {}", e)
        };
        match (parsed.next(), parsed.next()) {
            (Some(fst), Some(snd)) => Ok(TraceAttr::new(fst, snd)),
            (Some(fst), None) => panic!("trace attribute macro needs to know what step to record as its second argument!"),
            (None, Some(snd)) => panic!("trace attribute macro needs a trace_mgr location as its first argument"),
            _ => panic!("trace attribute macro needs two arguments; a trace_mgr location, and a step. got neither.")
        }
   }
}


#[proc_macro_attribute]
pub fn trace(_attr : TokenStream, input : TokenStream) -> TokenStream {
    let attr_contents = parse_macro_input!(_attr as TraceAttr);
    let original_function = parse_macro_input!(input as syn::ItemFn);
    let new_token_stream = add_tracing_to_item_fn(attr_contents, original_function);

    TokenStream::from(quote! {
        #new_token_stream
    })
}




#[proc_macro_attribute]
pub fn is_step(_attr : TokenStream, input : TokenStream) -> TokenStream {

    let mut as_enum = parse_macro_input!(input as syn::ItemEnum);

    // Collect the set of "short" names to use
    let short_set = crate::step_derive::collect_short_attrs(&mut as_enum);
    // Generate function to output short names for printing
    let short_name_getters = crate::step_derive::mk_name_getters_short(short_set);
    let name_getters = crate::step_derive::mk_name_getters2(&as_enum);
    let cnstr_impls = crate::step_derive::derive_cnstrs2(&as_enum);

    TokenStream::from(quote! {
        #as_enum
        #short_name_getters
        #name_getters
        #(#cnstr_impls)*
    })
}
