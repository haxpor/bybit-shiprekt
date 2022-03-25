/// Convenient macro to accept `OperationError` with optional error message
/// formed through variadic argument formatting to be printed alongside the
/// default error message from such former type.
#[macro_export]
macro_rules! errprint_exit1 {
    ($err:expr, $($args:expr),*) => {{
        let str_formed = std::fmt::format(format_args!($($args),*));
        eprintln!("{}", $err(Some(str_formed)));
        std::process::exit(1);
    }}
}
