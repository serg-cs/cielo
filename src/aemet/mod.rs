mod archive;
mod beaches;
mod client;
mod decoding;
mod fire;
mod forecast;
mod models;
mod sources;
mod tree;
mod warnings;

#[cfg(test)]
mod tests;

pub(crate) use client::AemetClient;
pub(crate) use fire::FireForecast;
pub(crate) use models::{
    Accumulation, AemetProduct, BeachForecast, DailyForecast, DailyRange, EntryFailure, FireDanger,
    HourValues, HourlyForecast, MunicipalityTimeZone, PeriodValues, WarningLevel, WeatherWarning,
};

#[cfg(test)]
pub(crate) use models::{DayForecast, ProbabilityWindow, SunTimes};
