extern crate proc_macro;
use proc_macro::TokenStream;

use std::collections::HashSet;
use proc_macro2::Ident as Ident2;
use quote::{ quote, format_ident };
use syn::{ parse_macro_input, 
           parse_quote, 
           visit_mut::VisitMut, 
           ItemFn, 
           Stmt };

mod helpers;

fn get_default_blacklist() -> Vec<syn::Path> {
        vec![
            parse_quote!(OffsetCache),
            parse_quote!(expr::OffsetCache),
            parse_quote!(crate::expr::OffsetCache),
            parse_quote!(Arc<RwLock<Env>>),
            parse_quote!(sync::Arc<RwLock<Env>>),
            parse_quote!(std::sync::Arc<RwLock<Env>>),
            parse_quote!(std::sync::Arc<lock_api::rwlock::RwLock<parking_lot::raw_rwlock::RawRwLock, env::Env>>),
            parse_quote!(Env),
            parse_quote!(env::Env),
            parse_quote!(crate::env::Env),
            parse_quote!(TypeChecker),
            parse_quote!(tc::TypeChecker),
            parse_quote!(crate::tc::TypeChecker),
            parse_quote!(CompiledModification),
            parse_quote!(env::CompiledModification),
            parse_quote!(crate::env::CompiledModification),
            parse_quote!(ReductionCache),
            parse_quote!(reduction::ReductionCache),
            parse_quote!(crate::reduction::ReductionCache),
        ]
}


/// For any function F decoarted with the #[tracing] attribute, we modify the 
/// original function, and then create a new function called F_2__.
struct TraceMacroState1 {
    trace_items : HashSet<Ident2>,
    type_blacklist : HashSet<syn::Path>,
    item_name : Ident2,
    fn_arg_infos : Vec<(syn::Pat, syn::Type, Ident2)>,
    trace_self : bool
}

impl TraceMacroState1 {
    pub fn new(trace_self : bool, trace_items : HashSet<Ident2>, _type_blacklist : Option<HashSet<syn::Path>>, f : &ItemFn) -> Self {


        // used to tag the `Op` node as IE `whnf_core ...`
        let item_name = f.sig.ident.clone();

        // get the name and type of each of the function's arguments
        // so we can add the code needed to trace them.
        let fn_arg_infos = f.sig.inputs.iter().filter_map(|item| {
            match item {
                syn::FnArg::Typed(syn::PatType { pat, ty, .. }) => {
                    match pat.as_ref() {
                        syn::Pat::Ident(pat_ident) => {
                            let lhs = pat.as_ref().clone();
                            let rhs = pat_ident.ident.clone();
                            Some((lhs, ty.as_ref().clone(), rhs))
                        }
                        _ => None
                    }
                },
                _ => None
            }
        }).collect::<Vec<(syn::Pat, syn::Type, syn::Ident)>>();

        let  blacklisted_types : Vec<syn::Path> = get_default_blacklist();

        let type_blacklist = {
            let mut set = _type_blacklist.unwrap_or_else(|| HashSet::new());
            for elem in blacklisted_types {
                set.insert(elem);
            };
            set
        };

        TraceMacroState1 {
            trace_items,
            type_blacklist,
            item_name,
            fn_arg_infos,
            trace_self
        }
    }

// The old function body becomes a block within the new tracing function.
// that gets surrounded by the tracing stuff.

    pub fn add_tracing_then_visit(&mut self, ifn : &mut syn::ItemFn) {
        self.add_tracing(ifn);
        self.visit_item_fn_mut(ifn);
    }

    pub fn add_tracing(&mut self, ifn : &mut syn::ItemFn) {
        // For functions with `core` in their method name, we add 
        // allow dead code, since in cases where both the parent and core 
        // function are decorated with a tracing attribute, the core method
        // will effectively become dead code (since the new tracing parent
        // only ever calls the `_2__` method)
        if ifn.sig.ident.to_string().contains("core") {
            let allow_unused_attr : syn::Attribute = parse_quote! {
                #[allow(dead_code)]
            };
            ifn.attrs.push(allow_unused_attr);
        } 

        // contents of old function block bound to a variable.
        let inner_block = ifn.block.clone();
        let inner_block_closure_local : syn::Stmt = parse_quote! {
            let mut inner_block_closure__ = || #inner_block;
        };


        let mut outer_block_stmts = Vec::new();

        let tracing_imports_stmts : syn::Item = syn::Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use crate::tracing::{ TraceData, HasInsertItem };
        });
        outer_block_stmts.push(syn::Stmt::Item(tracing_imports_stmts));


        let ident_literal = proc_macro2::Literal::string(&self.item_name.to_string());

        // let self_ident = "whnf_core";
        let self_ident_stmt : Stmt = parse_quote! {
            let self_ident : &'static str = #ident_literal;
        };
        outer_block_stmts.push(self_ident_stmt);

        let new_trace_data_stmt : syn::Stmt = parse_quote! {
            let mut trace_data__ : crate::tracing::TraceData = crate::tracing::TraceData::new();
        };
        outer_block_stmts.push(new_trace_data_stmt);

        // let new_root_op__ = trace_data__.new_root_op(self_idx);
        let new_op_stmt : syn::Stmt = parse_quote! {
            let mut this_op_idx__ : crate::tracing::OpIdx = trace_data__.new_root_op(self_ident);
        };
        outer_block_stmts.push(new_op_stmt);

        // Op's parent is already set
        let assert_parent_set_stmt : syn::Stmt = parse_quote! {
            match trace_data__.get_current_parent_op() {
                Some(x) if x == this_op_idx__ => (),
                Some(y) => panic!("parent op's current parent was Some(x) where x != this_op_idx"),
                None => panic!("should never have `None` parent op!")
            }
        };
        outer_block_stmts.push(assert_parent_set_stmt.clone());

        // make code to declare/insert function arguments and put them 
        // in the Op's trace data.
        // If the type is something in the blacklist (like offset_cache)
        // don't trace it.

        if self.trace_self {

            let clone_ident = format_ident!("self_arg_clone__");
            let item_idx_ident = format_ident!("self_arg_idx__");

            let trace_self_stmt =  parse_quote! {
                #[allow(unused_variables)]
                let #item_idx_ident = {
                    let #clone_ident = self;
                    let #item_idx_ident = trace_data__.insert_item(#clone_ident);
                    trace_data__.push_arg(this_op_idx__, #item_idx_ident);
                    #item_idx_ident
                };
            };
            outer_block_stmts.push(trace_self_stmt);
        }

        for (p, ty, id) in self.fn_arg_infos.clone().into_iter() {
            if !(self.type_blacklist.contains(&crate::helpers::get_collect_type(&ty))) {
               let as_stmt = crate::helpers::make_local_for_arg(p, id, &ty);
               outer_block_stmts.push(as_stmt);
            } else {
                continue
            }
        }

        // set op args, and set stmt to trace return value.
        //unimplemented!();

        outer_block_stmts.push(inner_block_closure_local);
        let inner_block_result_stmt : syn::Stmt = parse_quote! {
            let inner_block_result__1 = inner_block_closure__();
        };
        outer_block_stmts.push(inner_block_result_stmt);

        outer_block_stmts.push(assert_parent_set_stmt);

        let insert_ret_val_stmt : Stmt = parse_quote! {
            let ret_val_idx__ = trace_data__.push_ret_val(this_op_idx__, inner_block_result__1.clone());
        };
        outer_block_stmts.push(insert_ret_val_stmt);


        // Make a literal stmt of `inner_block_result__` that just returns whatever
        // the `inner_block_result__` block got as a return value.
        let return_result_path : syn::Expr = syn::Expr::Path(parse_quote!(inner_block_result__1));
        outer_block_stmts.push(syn::Stmt::Expr(return_result_path));

        std::mem::replace(&mut ifn.block.stmts, outer_block_stmts);
    }
}

// The only node sorts we want to universally modify are function and method calls
// to functions that are supposed to be tracing. We want to recursively walk the function
// and change all occurences of those functions to be occurences of their tracing
// version and add the trace_data__ as an argument.
impl VisitMut for TraceMacroState1 {
    fn visit_expr_call_mut(&mut self, b : &mut syn::ExprCall) {
        match b.func.as_mut() {
            syn::Expr::Path(expr_path) => {
                let stock_fn_name = expr_path.path.segments.last().cloned().map(|x| x.ident).expect("visi_expr_call_mut got empty path segment");
                if self.trace_items.contains(&stock_fn_name) {
                    let new_fn_name = format_ident!("{}_2__", stock_fn_name);
                    let tracer_arg : syn::ExprReference = parse_quote!(&mut trace_data__);
                    let tracer_arg_as_expr : syn::Expr = syn::Expr::Reference(tracer_arg);

                   // changes IE `crate::tc::whnf_core` to `crate::tc::whnf_core_2__`
                    expr_path.path.segments.last_mut().map(|pseg| std::mem::replace(&mut pseg.ident, new_fn_name));
                    b.args.push(tracer_arg_as_expr);

                } else {
                    // call to a method that's not supposed to be tracked.
                    return
                }
            },
            _ => unimplemented!("expected a path for method name in visit_expr_call_mut")
        };
    }

    fn visit_expr_method_call_mut(&mut self, b : &mut syn::ExprMethodCall) {
        if self.trace_items.contains(&b.method) {
            let new_method_name = format_ident!("{}_2__", &b.method);
            let tracer_arg : syn::ExprReference = parse_quote!(&mut trace_data__);
            let tracer_arg_as_expr : syn::Expr = syn::Expr::Reference(tracer_arg);

            std::mem::replace(&mut b.method, new_method_name);
            b.args.push(tracer_arg_as_expr);
        } else {
            // Don't modify; not supposed to be tracing
            return
        }
    }
}



struct TraceMacroState2 {
    trace_items : HashSet<Ident2>,
    type_blacklist : HashSet<syn::Path>,
    item_name : Ident2,
    fn_arg_infos : Vec<(syn::Pat, syn::Type, Ident2)>,
    trace_self : bool
}

impl TraceMacroState2 {
    pub fn new(trace_self : bool, trace_items : HashSet<Ident2>, _type_blacklist : Option<HashSet<syn::Path>>, f : &ItemFn) -> Self {

        // used to tag the `Op` node as IE `whnf_core ...`
        let item_name = f.sig.ident.clone();

        // get the name and type of each of the function's arguments
        // so we can add the code needed to trace them.
        let fn_arg_infos = f.sig.inputs.iter().filter_map(|item| {
            match item {
                syn::FnArg::Typed(syn::PatType { pat, ty, .. }) => {
                    match pat.as_ref() {
                        syn::Pat::Ident(pat_ident) => {
                            let lhs = pat.as_ref().clone();
                            let rhs = pat_ident.ident.clone();
                            Some((lhs, ty.as_ref().clone(), rhs))
                        }
                        _ => None
                    }
                },
                _ => None
            }
        }).collect::<Vec<(syn::Pat, syn::Type, syn::Ident)>>();

        let  blacklisted_types : Vec<syn::Path> = get_default_blacklist();

        let type_blacklist = {
            let mut set = _type_blacklist.unwrap_or_else(|| HashSet::new());
            for elem in blacklisted_types {
                set.insert(elem);
            };
            set
        };

        TraceMacroState2 {
            trace_items,
            type_blacklist,
            item_name,
            fn_arg_infos,
            trace_self
        }
    }

    fn modify_fn_then_visit(&mut self, ifn : &mut syn::ItemFn) {
        self.swap_function_name(ifn);
        self.push_trace_data_arg(ifn);
        self.track_function_block(ifn);
        self.visit_item_fn_mut(ifn);
    }

    // unique to `2` code generation; 
    fn swap_function_name(&self, ifn : &mut syn::ItemFn) {
        let new_ident = format_ident!("{}_2__", &ifn.sig.ident);
        std::mem::replace(&mut ifn.sig.ident, new_ident);
    }

    // also unique to `2` code generation.
    fn push_trace_data_arg(&self, ifn : &mut syn::ItemFn) {
        let new_arg : syn::FnArg = parse_quote!(trace_data__ : &mut crate::tracing::TraceData);
        ifn.sig.inputs.push(new_arg);
    }

    fn track_function_block(&mut self, ifn : &mut syn::ItemFn) {
        // Take what used to be the old function body, wrap it in a giant block,
        // and make that the result of the new outer block.

        let inner_block = ifn.block.clone();

        let inner_block_closure_local : syn::Stmt = parse_quote! {
            let mut inner_block_closure__ = || #inner_block;
        };


        let mut outer_block_stmts = Vec::new();

        let tracing_imports_stmts : syn::Item = syn::Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use crate::tracing::{ TraceData, HasInsertItem };
        });
        outer_block_stmts.push(syn::Stmt::Item(tracing_imports_stmts));



        // Get the function's name so it can be used to label the Op
        let ident_literal = proc_macro2::Literal::string(&self.item_name.to_string());

        // 2. Get the parent id if it exists.
        let parent_idx_stmt : Stmt = parse_quote! {
            let parent_idx : std::option::Option<crate::tracing::OpIdx> = trace_data__.get_current_parent_op();
        };

        let self_ident_stmt : Stmt = parse_quote! {
            let self_ident : &'static str = #ident_literal;
        };

        // 3. Make the new Op item, (gets inserted automatically by the new methods)
        // and either make it a root item, or set its parent.

        // if None, you know this is supposed to be the root op.
        // if Some, it's Nonroot
        let new_op_stmt : Stmt = parse_quote! {
            let this_op_idx__ : crate::tracing::OpIdx = match parent_idx {
                Some(x) => trace_data__.new_nonroot_op(self_ident, x),
                None => panic!("a `.._2__` function should never become a root!")
                //None => trace_data__.new_root_op(self_ident)
            };
        };

        let set_new_parent_stmt : Stmt = parse_quote! {
            trace_data__.set_parent_as(this_op_idx__);
        };

        outer_block_stmts.push(parent_idx_stmt);
        outer_block_stmts.push(self_ident_stmt);
        outer_block_stmts.push(new_op_stmt);
        outer_block_stmts.push(set_new_parent_stmt);


        if self.trace_self {

            let clone_ident = format_ident!("self_arg_clone__");
            let item_idx_ident = format_ident!("self_arg_idx__");

            let trace_self_stmt =  parse_quote! {
                #[allow(unused_variables)]
                let #item_idx_ident = {
                    let #clone_ident = self;
                    let #item_idx_ident = trace_data__.insert_item(#clone_ident);
                    trace_data__.push_arg(this_op_idx__, #item_idx_ident);
                    #item_idx_ident
                };
            };
            outer_block_stmts.push(trace_self_stmt);
        }


        // make code to declare/insert function arguments
        // If the type is something in the blacklist (like offset_cache)
        // don't trace it.
        for (p, ty, id) in self.fn_arg_infos.clone().into_iter() {
            if !(self.type_blacklist.contains(&crate::helpers::get_collect_type(&ty))) {
               let as_stmt = crate::helpers::make_local_for_arg(p, id, &ty);
               outer_block_stmts.push(as_stmt);
            } else {
                continue
            }
        }

        outer_block_stmts.push(inner_block_closure_local);

        let inner_block_result_stmt : syn::Stmt = parse_quote! {
            let mut inner_block_result__ = inner_block_closure__();
        };
        outer_block_stmts.push(inner_block_result_stmt);

        let maybe_reset_parent_stmt : Stmt = parse_quote! {
            match parent_idx {
                Some(opid) => trace_data__.set_parent_as(opid),
                _ => assert!(trace_data__.op_is_root(this_op_idx__))
            } 
        };
        outer_block_stmts.push(maybe_reset_parent_stmt);


        let insert_ret_val_stmt : Stmt = parse_quote! {
            let ret_val_idx__ = trace_data__.push_ret_val(this_op_idx__, inner_block_result__.clone());
        };
        outer_block_stmts.push(insert_ret_val_stmt);


        let return_result_path : syn::Expr = syn::Expr::Path(parse_quote!(inner_block_result__));
        outer_block_stmts.push(syn::Stmt::Expr(return_result_path));

        std::mem::replace(&mut ifn.block.stmts, outer_block_stmts);
    }

}


// For the `_2__` function's code generation, the only sorts we want to univerally
// modify are function and method calls (to change any non-tracking calls that
// should be tracking to their `_2__` tracking versions)
impl VisitMut for TraceMacroState2 {
    fn visit_expr_call_mut(&mut self, b : &mut syn::ExprCall) {
        match b.func.as_mut() {
            syn::Expr::Path(expr_path) => {
                let stock_fn_name = expr_path.path.segments.last().cloned().map(|x| x.ident).expect("visi_expr_call_mut got empty path segment");
                if self.trace_items.contains(&stock_fn_name) {
                    let new_fn_name = format_ident!("{}_2__", stock_fn_name);
                    let tracer_arg : syn::ExprPath = parse_quote!(trace_data__);
                    let tracer_arg_as_expr : syn::Expr = syn::Expr::Path(tracer_arg);

                   // changes IE `crate::tc::whnf_core` to `crate::tc::whnf_core_2__`
                    expr_path.path.segments.last_mut().map(|pseg| std::mem::replace(&mut pseg.ident, new_fn_name));
                    b.args.push(tracer_arg_as_expr);

                } else {
                    // call to a method that's not supposed to be tracked.
                    return
                }
            },
            _ => unimplemented!("expected a path for method name in visit_expr_call_mut")
        };
    }

    fn visit_expr_method_call_mut(&mut self, b : &mut syn::ExprMethodCall) {
        if self.trace_items.contains(&b.method) {
            let new_method_name = format_ident!("{}_2__", &b.method);
            let tracer_arg : syn::Expr = syn::Expr::Path(parse_quote!(trace_data__));
            std::mem::replace(&mut b.method, new_method_name);
            b.args.push(tracer_arg);
        } else {
            // Don't modify; not supposed to be tracing
            return
        }
    }


}



#[proc_macro_attribute]
pub fn tracing(_ : TokenStream, input : TokenStream) -> TokenStream {
    let trace_list = crate::helpers::read_trace_list();

    let blacklist : Option<HashSet<syn::Path>> = crate::helpers::try_read_type_blacklist();

    let input_clone = input.clone();
    
    // becomes decoarted base function
    let mut fn1 = parse_macro_input!(input_clone as ItemFn);

    // becomes .._2__ function
    let mut fn2  = parse_macro_input!(input as ItemFn);

    let mut trace_state1 = TraceMacroState1::new(false, trace_list.clone(), blacklist.clone(), &fn1);
    let mut trace_state2 = TraceMacroState2::new(false, trace_list.clone(), blacklist.clone(), &fn2);
    trace_state1.add_tracing_then_visit(&mut fn1);
    trace_state2.modify_fn_then_visit(&mut fn2);

    TokenStream::from(quote! {
        #fn1
        #fn2
    })
}

#[proc_macro_attribute]
pub fn tracing_w_self(_ : TokenStream, input : TokenStream) -> TokenStream {
    let trace_list = crate::helpers::read_trace_list();

    let blacklist : Option<HashSet<syn::Path>> = crate::helpers::try_read_type_blacklist();

    let input_clone = input.clone();
    
    // becomes decoarted base function
    let mut fn1 = parse_macro_input!(input_clone as ItemFn);

    // becomes .._2__ function
    let mut fn2  = parse_macro_input!(input as ItemFn);

    let mut trace_state1 = TraceMacroState1::new(true, trace_list.clone(), blacklist.clone(), &fn1);
    let mut trace_state2 = TraceMacroState2::new(true, trace_list.clone(), blacklist.clone(), &fn2);
    trace_state1.add_tracing_then_visit(&mut fn1);
    trace_state2.modify_fn_then_visit(&mut fn2);

    TokenStream::from(quote! {
        #fn1
        #fn2
    })
}


