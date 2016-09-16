use log;
use nix;
use std::{error, fmt, io, result};
use std::boxed::Box;

pub struct Error {
    pub desc: String,
    pub cause: Option<Box<error::Error>>,
}

pub type Result<T> = result::Result<T, Error>;

macro_rules! errfmt {
    ($msg:expr) => (Err(Error::from($msg.to_string())));
    ($err:expr, $msg:expr) => (Err(Error::from(($err, $msg.to_string()))));
    ($err:expr, $fmt:expr, $($arg:tt)+) => (Err(Error::from(($err, format!($fmt, $($arg)+)))));
}

macro_rules! unsafe_try {
    ( $x:expr, $($arg:tt)+ ) => {{
        let ret = unsafe { $x };

        if ret < 0 {
            return errfmt!(nix::Error::Sys(nix::Errno::last()), $($arg)+);
        } else {
            ret
        }
    }}
}

macro_rules! tryfmt {
    ($result:expr, $($arg:tt)+)  => (match $result {
            Ok(val) => val,
            Err(err) => {
                return errfmt!(err, $($arg)+)
            }
    })
}

impl error::Error for Error {
    fn description(&self) -> &str {
        return &*self.desc;
    }

    fn cause(&self) -> Option<&error::Error> {
        self.cause.as_ref().map(|e| &**e)
    }
}

macro_rules! from {
    ($error:ty) =>(impl From<($error, String)> for Error {
        fn from((error, desc): ($error, String)) -> Error {
            Error {
                desc: format!("{}: {}", desc, error),
                cause: Some(Box::new(error)),
            }
        }
    })
}

from!(io::Error);
from!(nix::Error);
from!(log::SetLoggerError);

impl From<(Error, String)> for Error {
    fn from((error, desc): (Error, String)) -> Error {
        Error {
            desc: format!("{}: {}", desc, error),
            cause: match error.cause {
                Some(cause) => Some(cause),
                None => None,
            },
        }
    }
}

impl From<String> for Error {
    fn from(desc: String) -> Error {
        Error {
            desc: desc,
            cause: None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
        (self as &error::Error).description().fmt(f)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
        fmt::Display::fmt(self, f)
    }
}
