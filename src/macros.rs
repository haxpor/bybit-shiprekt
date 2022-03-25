/// Convenient macro to just `eprint` string format, then exit with error code 1.
#[macro_export]
macro_rules! eprint_exit1 {
    ($($args:expr),*) => {{
        let str_formed = std::fmt::format(format_args!($($args),*));
        eprintln!("{}", str_formed);
        std::process::exit(1);
    }}
}

/// Convenient macro to accept `OperationError` with optional error message
/// formed through variadic argument formatting to be printed alongside the
/// default error message from such former type.
///
/// Required that result of input expression implements `std::fmt::Display` trait.
#[macro_export]
macro_rules! errprint_exit1 {
    ($err:expr) => {{
        eprintln!("{}", $err);
        std::process::exit(1);
    }};

    ($err:expr, $($args:expr),+) => {{
        let str_formed = std::fmt::format(format_args!($($args),+));
        eprintln!("{}", $err(Some(str_formed)));
        std::process::exit(1);
    }};
}

/// Convenient macro to return `Err<OperationError>` from the context of the code.
/// It's mostly useful when use with match arm.
#[macro_export]
macro_rules! ret_err {
    ($err:expr) => {{
        return Err($err(None));
    }};

    ($err:expr, $($args:expr),+) => {{
        let str_formed = std::fmt::format(format_args!($($args),+));
        return Err($err(Some(str_formed)));
    }}
}
