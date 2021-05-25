use simple_error::SimpleError;
use std::result;

pub type Result<T> = result::Result<T, SimpleError>;
