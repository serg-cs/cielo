mod application;
pub(crate) mod data;
mod files;
mod publisher;

#[cfg(test)]
mod tests;

const GENERATOR_IDENTITY: &str = "cielo";

pub(crate) use application::generate_application;
pub(crate) use data::generate_weather_data;
