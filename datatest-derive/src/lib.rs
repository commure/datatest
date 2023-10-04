#![recursion_limit = "128"]
#![deny(unused_must_use)]
extern crate proc_macro;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use std::collections::HashMap;
use syn::parse::{Parse, ParseStream, Result as ParseResult};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{braced, parse_macro_input, FnArg, Ident, ItemFn, Pat, PatIdent, PatType, Type, Token};

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
            let _not = input.parse::<syn::token::Not>()?;
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

        let args: Punctuated<TemplateArg, Comma> = content.parse_terminated(TemplateArg::parse, Token![,])?;
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

enum Registration {
    /// Register test cases via "global" constructors (https://crates.io/crates/ctor)
    Ctor,
    /// Register test cases via `#[test_case]` attribute (requires `custom_test_frameworks` unstable
    /// feature).
    Nightly,
}

/// Wrapper that turns on behavior that works on stable Rust.
#[proc_macro_attribute]
pub fn files_ctor_registration(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    guarded_test_attribute(
        args,
        func,
        Ident::new("files_ctor_internal", Span::call_site()),
    )
}

/// Wrapper that turns on behavior that works only on nightly Rust.
#[proc_macro_attribute]
pub fn files_test_case_registration(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    guarded_test_attribute(
        args,
        func,
        Ident::new("files_test_case_internal", Span::call_site()),
    )
}

#[proc_macro_attribute]
pub fn files_ctor_internal(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    files_internal(args, func, Registration::Ctor)
}

#[proc_macro_attribute]
pub fn files_test_case_internal(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    files_internal(args, func, Registration::Nightly)
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
/// cases when function takes argument as a slice (`&str`, `&[u8]`) and for cases when function takes
/// argument as owned (`String`, `Vec<u8>`).
///
/// The difficulty here is that for owned arguments we can create value and just pass it down to the
/// function. However, for arguments taking slices, we need to store value somewhere on the stack
/// and pass a reference.
///
/// I could have made this proc macro to handle these cases explicitly and generate a different
/// code, but I decided to not add a complexity of type analysis to the proc macro and use traits
/// instead. See `datatest::TakeArg` and `datatest::DeriveArg` to see how this mechanism works.
fn files_internal(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
    channel: Registration,
) -> proc_macro::TokenStream {
    let mut func_item: ItemFn = parse_macro_input!(func as ItemFn);
    let args: FilesTestArgs = parse_macro_input!(args as FilesTestArgs);
    let info = handle_common_attrs(&mut func_item, false);
    let func_ident = &func_item.sig.ident;
    let func_name_str = func_ident.to_string();
    let desc_ident = Ident::new(&format!("__TEST_{}", func_ident), func_ident.span());
    let trampoline_func_ident = Ident::new(
        &format!("__TEST_TRAMPOLINE_{}", func_ident),
        func_ident.span(),
    );
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
    for (mut idx, arg) in func_item.sig.inputs.iter().enumerate() {
        match match_arg(arg) {
            Some((pat_ident, ty)) => {
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
                        ::datatest::__internal::TakeArg::take(&mut <#ty as ::datatest::__internal::DeriveArg>::derive(&paths_arg[#idx]))
                    })
                } else {
                    return Error::new(pat_ident.span(), "mapping is not defined for the argument")
                        .to_compile_error()
                        .into();
                }
            }
            None => {
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

    let (kind, bencher_param) = if info.bench {
        (
            quote!(BenchFn),
            quote!(bencher: &mut ::datatest::__internal::Bencher,),
        )
    } else {
        (quote!(TestFn), quote!())
    };

    let registration = test_registration(channel, &desc_ident);
    let output = quote! {
        #registration
        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        static #desc_ident: ::datatest::__internal::FilesTestDesc = ::datatest::__internal::FilesTestDesc {
            name: concat!(module_path!(), "::", #func_name_str),
            ignore: #ignore,
            root: #root,
            params: &[#(#params),*],
            pattern: #pattern_idx,
            ignorefn: #ignore_func_ref,
            testfn: ::datatest::__internal::FilesTestFn::#kind(#trampoline_func_ident),
            source_file: file!(),
        };

        #[automatically_derived]
        #[allow(non_snake_case)]
        fn #trampoline_func_ident(#bencher_param paths_arg: &[::std::path::PathBuf]) {
            let result = #func_ident(#(#invoke_args),*);
            ::datatest::__internal::assert_test_result(result);
        }

        #func_item
    };
    output.into()
}

fn match_arg(arg: &FnArg) -> Option<(&PatIdent, &Type)> {
    if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
        if let Pat::Ident(pat_ident) = pat.as_ref() {
            return Some((pat_ident, ty));
        }
    }
    None
}

enum ShouldPanic {
    No,
    Yes,
    YesWithMessage(String),
}

struct FuncInfo {
    ignore: bool,
    bench: bool,
    should_panic: ShouldPanic,
}

/// Only allows certain attributes (`#[should_panic]`, for example) when used against a "regular"
/// test `#[test]`.
fn handle_common_attrs(func: &mut ItemFn, regular_test: bool) -> FuncInfo {
    // Remove #[test] attribute as we don't want standard test framework to handle it!
    // We allow #[test] to be used to improve IDE experience (namely, IntelliJ Rust), which would
    // only allow you to run test if it is marked with `#[test]`
    let test_pos = func
        .attrs
        .iter()
        .position(|attr| attr.path().is_ident("test"));
    if let Some(pos) = test_pos {
        func.attrs.remove(pos);
    }

    // Same for #[bench]
    let bench_pos = func
        .attrs
        .iter()
        .position(|attr| attr.path().is_ident("bench"));
    if let Some(pos) = bench_pos {
        func.attrs.remove(pos);
    }

    // Allow tests to be marked as `#[ignore]`.
    let ignore_pos = func
        .attrs
        .iter()
        .position(|attr| attr.path().is_ident("ignore"));
    if let Some(pos) = ignore_pos {
        func.attrs.remove(pos);
    }

    let mut should_panic = ShouldPanic::No;
    if regular_test {
        // Regular tests support (on stable channel): allow `#[should_panic]`
        let should_panic_pos = func
            .attrs
            .iter()
            .position(|attr| attr.path().is_ident("should_panic"));
        if let Some(pos) = should_panic_pos {
            let attr = &func.attrs[pos];
            should_panic = parse_should_panic(attr);
            func.attrs.remove(pos);
        }
    }

    FuncInfo {
        ignore: ignore_pos.is_some(),
        bench: bench_pos.is_some(),
        should_panic,
    }
}

#[allow(clippy::collapsible_match)]
fn parse_should_panic(attr: &syn::Attribute) -> ShouldPanic {
    if let Err(err) = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("expected") {
            return match meta.value() {
                Ok(v) => Err(syn::Error::new(v.span(), v.to_string())),
                Err(err) => Err(err),
            };
        }
        Ok(())
    }) {
        return ShouldPanic::YesWithMessage(err.to_string());
    }

    ShouldPanic::Yes
}

/// Parse `#[data(...)]` attribute arguments. It's either a function returning
/// `Vec<datatest::DataTestCaseDesc<T>>` (where `T` is a test case type) or string literal, which
/// is interpreted as `datatest::yaml("<path>")`
#[allow(clippy::large_enum_variant)]
enum DataTestArgs {
    Literal(syn::LitStr),
    Expression(syn::Expr),
}

/// See `syn` crate documentation / sources for more examples.
impl Parse for DataTestArgs {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(syn::LitStr) {
            input.parse::<syn::LitStr>().map(DataTestArgs::Literal)
        } else {
            input.parse::<syn::Expr>().map(DataTestArgs::Expression)
        }
    }
}

/// Wrapper that turns on behavior that works on stable Rust.
#[proc_macro_attribute]
pub fn data_ctor_registration(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    guarded_test_attribute(
        args,
        func,
        Ident::new("data_ctor_internal", Span::call_site()),
    )
}

/// Wrapper that turns on behavior that works only on nightly Rust.
#[proc_macro_attribute]
pub fn data_test_case_registration(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    guarded_test_attribute(
        args,
        func,
        Ident::new("data_test_case_internal", Span::call_site()),
    )
}

#[proc_macro_attribute]
pub fn data_ctor_internal(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    data_internal(args, func, Registration::Ctor)
}

#[proc_macro_attribute]
pub fn data_test_case_internal(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    data_internal(args, func, Registration::Nightly)
}

fn data_internal(
    args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
    channel: Registration,
) -> proc_macro::TokenStream {
    let mut func_item = parse_macro_input!(func as ItemFn);
    let cases: DataTestArgs = parse_macro_input!(args as DataTestArgs);
    let info = handle_common_attrs(&mut func_item, false);
    let cases = match cases {
        DataTestArgs::Literal(path) => quote!(datatest::yaml(#path)),
        DataTestArgs::Expression(expr) => quote!(#expr),
    };
    let func_ident = &func_item.sig.ident;

    let func_name_str = func_ident.to_string();
    let desc_ident = Ident::new(&format!("__TEST_{}", func_ident), func_ident.span());
    let describe_func_ident = Ident::new(
        &format!("__TEST_DESCRIBE_{}", func_ident),
        func_ident.span(),
    );
    let trampoline_func_ident = Ident::new(
        &format!("__TEST_TRAMPOLINE_{}", func_ident),
        func_ident.span(),
    );

    let ignore = info.ignore;
    // FIXME: check file exists!
    let mut args = func_item.sig.inputs.iter();

    if info.bench {
        // Skip Bencher argument
        // FIXME: verify it is &mut Bencher
        args.next();
    }

    let arg = args.next();
    let ty = match arg {
        Some(FnArg::Typed(PatType { ty, .. })) => Some(ty.as_ref()),
        _ => None,
    };
    let (ref_token, ty) = match ty {
        Some(syn::Type::Reference(type_ref)) => (quote!(&), Some(type_ref.elem.as_ref())),
        _ => (TokenStream::new(), ty),
    };

    let (case_ctor, bencher_param, bencher_arg) = if info.bench {
        (
            quote!(::datatest::__internal::DataTestFn::BenchFn(Box::new(::datatest::__internal::DataBenchFn(#trampoline_func_ident, case)))),
            quote!(bencher: &mut ::datatest::__internal::Bencher,),
            quote!(bencher,),
        )
    } else {
        (
            quote!(::datatest::__internal::DataTestFn::TestFn(Box::new(move || #trampoline_func_ident(case)))),
            quote!(),
            quote!(),
        )
    };

    let registration = test_registration(channel, &desc_ident);
    let output = quote! {
        #registration
        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        static #desc_ident: ::datatest::__internal::DataTestDesc = ::datatest::__internal::DataTestDesc {
            name: concat!(module_path!(), "::", #func_name_str),
            ignore: #ignore,
            describefn: #describe_func_ident,
            source_file: file!(),
        };

        #[automatically_derived]
        #[allow(non_snake_case)]
        fn #trampoline_func_ident(#bencher_param arg: #ty) {
            let result = #func_ident(#bencher_arg #ref_token arg);
            ::datatest::__internal::assert_test_result(result);
        }

        #[automatically_derived]
        #[allow(non_snake_case)]
        fn #describe_func_ident() -> Vec<::datatest::DataTestCaseDesc<::datatest::__internal::DataTestFn>> {
            let result = #cases
                .into_iter()
                .map(|input| {
                    let case = input.case;
                    ::datatest::DataTestCaseDesc {
                        case: #case_ctor,
                        name: input.name,
                        location: input.location,
                    }
                })
                .collect::<Vec<_>>();
            assert!(!result.is_empty(), "no test cases were found!");
            result
        }

        #func_item
    };
    output.into()
}

fn test_registration(channel: Registration, desc_ident: &syn::Ident) -> TokenStream {
    match channel {
        // On nightly, we rely on `custom_test_frameworks` feature
        Registration::Nightly => quote!(#[test_case]),
        // On stable, we use `ctor` crate to build a registry of all our tests
        Registration::Ctor => {
            let registration_fn =
                syn::Ident::new(&format!("{}__REGISTRATION", desc_ident), desc_ident.span());
            let check_fn = syn::Ident::new(&format!("{}__CHECK", desc_ident), desc_ident.span());
            let tokens = quote! {
                #[automatically_derived]
                #[allow(non_snake_case)]
                #[datatest::__internal::ctor]
                fn #registration_fn() {
                    use ::datatest::__internal::RegistrationNode;
                    static mut REGISTRATION: RegistrationNode = RegistrationNode {
                        descriptor: &#desc_ident,
                        next: None,
                    };
                    // This runs only once during initialization, so should be safe
                    ::datatest::__internal::register(unsafe { &mut REGISTRATION });
                }

                // Make sure we our registry was actually scanned!
                // This would detect scenario where none of the ways are used to plug datatest
                // test runner (either by replacing the whole harness or by overriding test runner).
                // So, for every test we have registered, we make sure this test actually gets
                // executed.
                #[automatically_derived]
                #[allow(non_snake_case)]
                mod #check_fn {
                    #[datatest::__internal::dtor]
                    fn check_fn() {
                        ::datatest::__internal::check_test_runner();
                    }
                }
            };
            tokens
        }
    }
}

/// Replacement for the `#[test]` attribute that uses ctor-based test registration so it can be
/// used when the whole test harness is replaced.
#[proc_macro_attribute]
pub fn test_ctor_registration(
    _args: proc_macro::TokenStream,
    func: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut func_item = parse_macro_input!(func as ItemFn);
    let info = handle_common_attrs(&mut func_item, true);
    let func_ident = &func_item.sig.ident;
    let func_name_str = func_ident.to_string();
    let desc_ident = Ident::new(&format!("__TEST_{}", func_ident), func_ident.span());

    let ignore = info.ignore;
    let should_panic = match info.should_panic {
        ShouldPanic::No => quote!(::datatest::__internal::RegularShouldPanic::No),
        ShouldPanic::Yes => quote!(::datatest::__internal::RegularShouldPanic::Yes),
        ShouldPanic::YesWithMessage(v) => {
            quote!(::datatest::__internal::RegularShouldPanic::YesWithMessage(#v))
        }
    };
    let registration = test_registration(Registration::Ctor, &desc_ident);
    let output = quote! {
        #registration
        #[automatically_derived]
        #[allow(non_upper_case_globals)]
        static #desc_ident: ::datatest::__internal::RegularTestDesc = ::datatest::__internal::RegularTestDesc {
            name: concat!(module_path!(), "::", #func_name_str),
            ignore: #ignore,
            testfn: || {
                let result = #func_ident();
                ::datatest::__internal::assert_test_result(result);
            },
            should_panic: #should_panic,
            source_file: file!(),
        };

        #func_item
    };

    output.into()
}

fn guarded_test_attribute(
    args: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
    implementation: Ident,
) -> proc_macro::TokenStream {
    let args: TokenStream = args.into();
    let header = quote! {
        #[cfg(test)]
        #[::datatest::__internal::#implementation(#args)]
    };
    let mut out: proc_macro::TokenStream = header.into();
    out.extend(item);
    out
}
