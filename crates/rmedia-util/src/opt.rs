//! Options and reflection system (AVOption and AVClass reproduction).

use std::str::FromStr;
use crate::error::{Result, RmediaError};

/// Types of options supported by the reflection system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionType {
    Flags,
    Int,
    Int64,
    Double,
    Float,
    String,
    Rational,
    Binary,
    Dict,
    PixelFormat,
    SampleFormat,
    Duration,
    Color,
    ChannelLayout,
    Bool,
}

/// Option descriptor (AVOption).
#[derive(Debug, Clone, PartialEq)]
pub struct AvOption {
    pub name: &'static str,
    pub help: &'static str,
    pub opt_type: OptionType,
    pub default_val: Option<&'static str>,
    pub min: f64,
    pub max: f64,
    pub flags: u32,
    pub unit: Option<&'static str>,
}

/// Class descriptor containing metadata and options (AVClass).
#[derive(Debug, Clone, PartialEq)]
pub struct AvClass {
    pub class_name: &'static str,
    pub options: Vec<AvOption>,
    pub version: (u32, u32, u32),
}

/// Trait for objects that expose options through the AvClass system.
pub trait OptionContext {
    /// Return the AvClass description of this object.
    fn get_class(&self) -> &AvClass;

    /// Set an option by string key and string value with validation.
    fn set_option(&mut self, name: &str, value: &str) -> Result<()>;

    /// Retrieve an option value by string key.
    fn get_option(&self, name: &str) -> Option<String>;
}

/// Parse and validate numeric value against bounds.
pub fn parse_and_validate_num<T: FromStr + PartialOrd + Copy>(
    val: &str,
    min: T,
    max: T,
) -> Result<T> {
    let parsed = val.parse::<T>().map_err(|_| {
        RmediaError::InvalidArgument(format!("Cannot parse numeric value '{}'", val))
    })?;
    if parsed < min || parsed > max {
        return Err(RmediaError::InvalidArgument(format!(
            "Value '{}' is out of range",
            val
        )));
    }
    Ok(parsed)
}
