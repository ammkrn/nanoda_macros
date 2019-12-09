A macro crate for [nanoda](https://github.com/ammkrn/nanoda).

### About

Right now it contains two attribute macros that allow users to obtain a trace of what steps the checker takes when building and ultimately checking terms. The two macros are `#[tracing]` and `#[tracing_w_self]` (the difference is explained in `How to use`). Decorating a function with one of these two attributes (after they're brought into scope) will automatically generate code that does one of two things :

1. If the decorated function is called by a function which is NOT being traced, then from the perspective of the trace, the current function call has no discernable parent operation and therefore becomes the root node of a new execution graph which will appear in the output.

2. If the decorated function is called by a function that IS being traced, the calling function is therefore recognized as a parent operation, and the callee is properly traced as the child of some existing execution graph.

The trace output is closely modeled on the Lean export format, though is more complex since we're keeping track of a broader body of information. Also, in contrast to the Lean export format which only needs to talk about data, we also need to talk about execution steps, so in addition to `Item`s, we output another group of elements that we're calling `Op`s, which are the actual execution steps. Each execution graph created during type checking will output any unique terms (terms created during checking that are not in the Lean export file), and then the list of operations performed during typechecking. Finally, the set of items universally (similar to the original Lean export file) is output.

Terms are tracked with natural number identifiers similar to the Lean export format. Terms that are part of the original Lean item set are written just as natural numbers. Terms that are unique to some trace are prefaced with a `!` (IE `!129`), and `Op` identifiers are prefaced with `$` (IE `$999`).

Each `Op`/execution step includes the following information in this order :
+ It's UID (unique identifier; a natural number)
+ The name of the operation (IE a call to `whnf` will be marked as such)
+ The UID of its parent if non-root (none if its a root node)
+ The UIDs of its arguments in parenthesis
+ The UID of its return value
+ The UIDs of any child operations in its execution graph in square brackets.

For example :<br/>
`$22 "whnf_core" $13 (91, !11, 83) !29 [$23, $24]`

The "syntax" for Items includes that of the Lean export format, in addition to :
+ Local Exprs  (UID, tag, serial, biner name UID, binder type UID)<br/>
`UID #ELO BinderStyle int UID UID`  

+ Sequences of items (UID, tag, list of UIDs)<br/>
`UID #SEQ UID*`

+ Representation of the Some(t) variant of Option<T> types (UID, tag, UID of inner `t` value)<br/>
`UID #SOME UID`

+ Pairs of UIDs to represent 2-tuples (UID, tag, fst UID, snd UID)<br/>
`UID #TUP UID UID`

+ ReductionRule (UID, tag, name UID, lhs/type UID, rhs/val UID, var_bound, args_size)<br/>
`UID #RR UID UID UID int int`

+ Declaration (UID, tag, name UID, seq of universe params UID, type UID, def height)<br/>
`UID #DEC UID UID UID int`

+ Compiled Axiom (UID, tag, type UID)<br/>
`UID #CAX UID`

+ Compiled Definition (UID, tag, declaration UID, reduction rule UID, type UID, value UID)<br/>
`UID #CDEF UID UID UID UID`

+ Compiled Quot (UID, tag, seq of declarations UID, reduction rule UID)<br/>
`UID #CQUOT UID UID`

+ Compiled Inductive (UID, tag, Declaration uid, seq of declarations UID, declaration UID, seq of reduction rules UID)<br/>
`UID #CIND UID UID UID UID`

And "constant" items that are only ever inserted once and then get referenced by UID :<br/>

+ The anonymous name<br/>
`UID Anon`

+ Level 0<br/>
`UID Zero`

+ The `None` variant of any Option<T><br/>
`UID #NONE`

+ True boolean<br/>
`UID #TT`

+ False boolean<br/>
`UID #FF`

+ True const reduction flag<br/>
`UID #FLAGT`

+ False const reduction flag<br/>
`UID #FLAGF`

+ Unit (this was needed for us to be generic over function return values, since some of the functions
you may want to trace have no return value)<br/>
`UID #UNIT`

### How to use

1. Add the crate to the Cargo.toml of `nanoda`
2. Enable the `tracing` feature by building with --features tracing
3. Import the appropriate macro in whatever module.
4. Decorate any function call with either `#[tracing]` or `#[tracing_w_self]`. 
5. For any decorated function, you also need to put the function's name in the config/traced_items.txt file, one name per line. We need this so that decorated functions can know what other functions have been decorated (more info in the `How it works` section).

The difference between the two macros is whether or not the `self` argument to the function is tracked. There are certain types that we don't want to track, because we don't want to print them repeatedly (or at all), or because they're larger global terms that get printed as one piece later. For everything but the `self` argument to a function, we can detect when a term of one of these types is in the argument position and just not track it, but the `self` argument doesn't expose any type information to the procedural macro, so we have to put that burden on the user. For example, in cases where you want to trace the execution of the `instantiate` function, you would use `#[tracing_w_self]`, since the `self` argument is the Expr being instantiated. However, for something like `infer`, you would use `#[tracing]`, since the `self` argument is a TypeChecker, which we definitely don't want to log. There's a set of argument types we want to ignore hard-coded into the macro, so if you try to use `#[tracing_w_self]` in a position where you shouldn't, you'll get an error, meaning you should just use `#[tracing]`. In the event that I forgot one, or you want to trace something weird or something you've added to the source code, you can add to the list of blacklisted types by editing the config/type_blacklist.txt file, putting one type per line.


### How it works

For any given funciton `f` decorated with one of the two `tracing` attributes, the original function is modified to produce the root of a new execution graph, and a new sister function `f_2__` is produced that takes an explicit argument of some existing execution graph, and represents calls to `f` in anything other than the root position. To do this, any decorated function that calls `f` has its call to `f` replaced by `f_2__(.., TraceData)` where TraceData is the existing execution graph. The reason users need to specify what functions they're tracing in the `traced_items.txt` file is so that during code generation, we can discern what function calls need to be replaced, since IE code generation for `whnf_core` can't (without help) discern whether or not `instantiate` is decorated or not. The best way to get a hands on feel for exactly what's generated is by using [cargo-expand](https://github.com/dtolnay/cargo-expand), which will show you what the source code looks like after the macro does its work.












