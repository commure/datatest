#![recursion_limit = "128"]
#![deny(unused_must_use)]
extern crate proc_macro;

#[macro_use]
extern crate syn;
#[macro_use]
extern crate quote;
extern crate proc_macro2;

use proc_macro2::{Span, TokenStream};
use std::collections::HashMap;
use syn::parse::{Parse, ParseStream, Result as ParseResult};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{ArgCaptured, FnArg, Ident, ItemFn, Pat};

type Error = syn::parse::Error;

struct TemplateArg {
    ident: syn::Ident,
    is_pattern: bool,
    ignore_fn: Option<syn::Path>,
    value: syn::LitStr,
}

impl Parse for TemplateArg {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let mut ignore_fn = None;
        let ident = input.parse::<syn::Ident>()?;

        let is_pattern = if input.peek(syn::token::In) {
            let _in = input.parse::<syn::token::In>()?;
            true
        } else {
            let _eq = input.parse::<syn::token::Eq>()?;
            false
        };
        let value = input.parse::<syn::LitStr>()?;
        if is_pattern && input.peek(syn::token::If) {
            let _if = input.parse::<syn::token::If>()?;
            let _not = input.parse::<syn::token::Bang>()?;
            ignore_fn = Some(input.parse::<syn::Path>()?);
        }
        Ok(Self {
            ident,
            is_pattern,
            ignore_fn,
            value,
        })
    }
}

/// Parse `#[file_test(...)]` attribute arguments
/// The syntax is the following:
///
/// ```ignore
/// #[files("<root>", {
///   <arg_name> in "<regexp>",
///   <arg_name> in "<template>",
/// }]
/// ```
struct FilesTestArgs {
    root: String,
    args: HashMap<Ident, TemplateArg>,
}

/// See `syn` crate documentation / sources for more examples.
impl Parse for FilesTestArgs {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let root = input.parse::<syn::LitStr>()?;
        let _comma = input.parse::<syn::token::Comma>()?;
        let content;
        let _brace_token = braced!(content in input);

        let args: Punctuated<TemplateArg, Comma> = content.parse_terminated(TemplateArg::parse)?;
        let args = args
            .into_pairs()
            .map(|p| {
                let value = p.into_value();
                (value.ident.clone(), value)
            })
            .collect();

        Ok(Self {
            root: root.value(),
            args,
        })
    }
}

/// Proc macro handling `#[files(...)]` syntax. This attribute defines rules for deriving
/// test function arguments from file paths. There are two types of rules:
/// 1. Pattern rule, `<arg_name> in "<regexp>"`
/// 2. Template rule, `<arg_name> = "regexp"`
///
/// There must be only one pattern rule defined in the attribute. It defines a regular expression
/// to run against all files found in the test directory.
///
/// Template rule defines rules how the name of the matched file is transformed to get related files.
///
/// This macro is responsible for generating a test descriptor (`datatest::FilesTestDesc`) based on the
/// `#[files(..)]` attribute attached to the test function.
///
/// There are four fields specific for these type of tests we need to fill in:
///
/// 1. `root`, which is the root directory to scan for the tests (relative to the root of the crate
/// with tests)
/// 2. `params`, slice of strings, each string is either a template or pattern assigned to the
/// function argument
/// 3. `pattern`, an index of the "pattern" argument (since exactly one is required, it is just an
/// index in the `params` array).
/// 4. `testfn`, test function trampoline.
///
/// Few words about trampoline function. Each test function could have a unique signature, depending
/// on which types it needs and which files it requires as an input. However, our test framework
/// should be capable of running these test functions via some standardized interface. This interface
/// is `fn(&[PathBuf])`. Each slice element matches test function argument (so length of this slice
/// is the same as amount of arguments test function has).
///
/// In addition to that, this trampoline function is also responsible for mapping `&PathBuf`
/// references into argument types. There is some trait magic involved to make code work for both
/// cases when function takes argument as a slice (`&str`, `&[u8`) and for cases when function takes
/// argument as owned (`String`, `Vec<u8>`).
///
/// The difficulty here is that for owned arguments we can create value and just pass it down to the
/// function. However, for arguments taking slices, we need to store value somewhere on the stack
/// and pass a reference.
///
/// I could have made this proc macro to handle these cases explicitly and generate a different
/// code, but I decided to not add a complexity of type analysis to the proc macro and use traits
/// instead. See `datatest::TakeArg` and `datatest::DeriveArg` to see how this mechanism works.
#[proc_macro_attribute]
#[allow(clippy::needless_pass_by_value)]
pub fn files(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut func_item = parse_macro_input!(func as ItemFn);
    let args: FilesTestArgs = parse_macro_input!(args as FilesTestArgs);

    let func_name_str = func_item.ident.to_string();
    let desc_ident = Ident::new(
        &format!("__TEST_{}", func_item.ident),
        func_item.ident.span(),
    );
    let trampoline_func_ident = Ident::new(
        &format!("__TEST_TRAMPOLINE_{}", func_item.ident),
        func_item.ident.span(),
    );

    let info = handle_common_attrs(&mut func_item);
    let ignore = info.ignore;

    let root = args.root;
    let mut pattern_idx = None;
    let mut params: Vec<String> = Vec::new();
    let mut invoke_args: Vec<TokenStream> = Vec::new();
    let mut ignore_fn = None;

    // Match function arguments with our parsed list of mappings
    // We do the following in this loop:
    // 1. For each argument we collect the corresponding template defined for that argument
    // 2. For each argument we collect piece of code to create argument from the `&[PathBuf]` slice
    // given to us by the test runner.
    // 3. Capture the index of the argument corresponding to the "pattern" mapping
    for (mut idx, arg) in func_item.decl.inputs.iter().enumerate() {
        match arg {
            FnArg::Captured(ArgCaptured {
                pat: Pat::Ident(pat_ident),
                ty,
                ..
            }) => {
                if info.bench {
                    if idx == 0 {
                        // FIXME: verify is Bencher!
                        invoke_args.push(quote!(#pat_ident));
                        continue;
                    } else {
                        idx -= 1;
                    }
                }

                if let Some(arg) = args.args.get(&pat_ident.ident) {
                    if arg.is_pattern {
                        if pattern_idx.is_some() {
                            return Error::new(arg.ident.span(), "two patterns are not allowed!")
                                .to_compile_error()
                                .into();
                        }
                        pattern_idx = Some(idx);
                        ignore_fn = arg.ignore_fn.clone();
                    }

                    params.push(arg.value.value());
                    invoke_args.push(quote! {
                        ::datatest::TakeArg::take(&mut <#ty as ::datatest::DeriveArg>::derive(&paths_arg[#idx]))
                    })
                } else {
                    return Error::new(pat_ident.span(), "mapping is not defined for the argument")
                        .to_compile_error()
                        .into();
                }
            }
            _ => {
                return Error::new(
                    arg.span(),
                    "unexpected argument; only simple argument types are allowed (`&str`, `String`, `&[u8]`, `Vec<u8>`, `&Path`, etc)",
                ).to_compile_error().into();
            }
        }
    }

    let ignore_func_ref = if let Some(ignore_fn) = ignore_fn {
        quote!(Some(#ignore_fn))
    } else {
        quote!(None)
    };

    if pattern_idx.is_none() {
        return Error::new(
            Span::call_site(),
            "must have exactly one pattern mapping defined via `pattern in r#\"<regular expression>\"`",
        )
            .to_compile_error()
            .into();
    }

    // So we can invoke original function from the trampoline function
    let orig_func_name = &func_item.ident;

    let (kind, bencher_param) = if info.bench {
        (quote!(BenchFn), quote!(bencher: &mut ::datatest::Bencher,))
    } else {
        (quote!(TestFn), quote!())
    };

    // Adding `#[allow(unused_attributes)]` to `#orig_func` to allow `#[ignore]` attribute
    let output = quote! {
        #[test_case]
        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        static #desc_ident: ::datatest::FilesTestDesc = ::datatest::FilesTestDesc {
            name: concat!(module_path!(), "::", #func_name_str),
            ignore: #ignore,
            root: #root,
            params: &[#(#params),*],
            pattern: #pattern_idx,
            ignorefn: #ignore_func_ref,
            testfn: ::datatest::FilesTestFn::#kind(#trampoline_func_ident),
        };

        #[automatically_derived]
        #[allow(non_snake_case)]
        fn #trampoline_func_ident(#bencher_param paths_arg: &[::std::path::PathBuf]) {
            let result = #orig_func_name(#(#invoke_args),*);
            datatest::assert_test_result(result);
        }

        #func_item
    };
    output.into()
}

struct FuncInfo {
    ignore: bool,
    bench: bool,
}

fn handle_common_attrs(func: &mut ItemFn) -> FuncInfo {
    // Remove #[test] attribute as we don't want standard test framework to handle it!
    // We allow #[test] to be used to improve IDE experience (namely, IntelliJ Rust), which would
    // only allow you to run test if it is marked with `#[test]`
    let test_pos = func
        .attrs
        .iter()
        .position(|attr| attr.path.is_ident("test"));
    if let Some(pos) = test_pos {
        func.attrs.remove(pos);
    }

    // Same for #[bench]
    let bench_pos = func
        .attrs
        .iter()
        .position(|attr| attr.path.is_ident("bench"));
    if let Some(pos) = bench_pos {
        func.attrs.remove(pos);
    }

    // Allow tests to be marked as `#[ignore]`.
    let ignore_pos = func
        .attrs
        .iter()
        .position(|attr| attr.path.is_ident("ignore"));
    if let Some(pos) = ignore_pos {
        func.attrs.remove(pos);
    }
    FuncInfo {
        ignore: ignore_pos.is_some(),
        bench: bench_pos.is_some(),
    }
}

#[proc_macro_attribute]
#[allow(clippy::needless_pass_by_value)]
pub fn data(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut func_item = parse_macro_input!(func as ItemFn);
    let root: syn::LitStr = parse_macro_input!(args as syn::LitStr);

    let func_name_str = func_item.ident.to_string();
    let desc_ident = Ident::new(
        &format!("__TEST_{}", func_item.ident),
        func_item.ident.span(),
    );
    let describe_func_ident = Ident::new(
        &format!("__TEST_DESCRIBE_{}", func_item.ident),
        func_item.ident.span(),
    );
    let trampoline_func_ident = Ident::new(
        &format!("__TEST_TRAMPOLINE_{}", func_item.ident),
        func_item.ident.span(),
    );

    let info = handle_common_attrs(&mut func_item);
    let ignore = info.ignore;

    // FIXME: check file exists!

    let orig_func_ident = &func_item.ident;
    let mut args = func_item.decl.inputs.iter();

    if info.bench {
        // Skip Bencher argument
        // FIXME: verify it is &mut Bencher
        args.next();
    }

    let arg = args.next();
    let ty = match arg {
        Some(FnArg::Captured(ArgCaptured { ty, .. })) => Some(ty),
        _ => None,
    };
    let (ref_token, ty) = match ty {
        Some(syn::Type::Reference(type_ref)) => (quote!(&), Some(type_ref.elem.as_ref())),
        _ => (TokenStream::new(), ty),
    };

    let (describe, bencher_param, bencher_arg) = if info.bench {
        (
            quote!(describe_bench),
            quote!(bencher: &mut ::datatest::Bencher,),
            quote!(bencher,),
        )
    } else {
        (quote!(describe_test), quote!(), quote!())
    };

    let output = quote! {
        #[test_case]
        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        static #desc_ident: ::datatest::DataTestDesc = ::datatest::DataTestDesc {
            name: concat!(module_path!(), "::", #func_name_str),
            ignore: #ignore,
            root: #root,
            describefn: #describe_func_ident,
        };

        #[automatically_derived]
        #[allow(non_snake_case)]
        fn #trampoline_func_ident(#bencher_param arg: #ty) {
            let result = #orig_func_ident(#bencher_arg #ref_token arg);
            datatest::assert_test_result(result);
        }

        #[automatically_derived]
        #[allow(non_snake_case)]
        fn #describe_func_ident(input: &str) -> Vec<::datatest::DataTestCaseDesc<::datatest::DataTestFn>> {
            ::datatest::#describe(input, #trampoline_func_ident)
        }

        #func_item
    };
    output.into()
}
