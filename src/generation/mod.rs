mod application;
mod files;
mod location_names;
mod publisher;
mod weather_data;

#[cfg(test)]
mod tests;

const GENERATOR_IDENTITY: &str = "cielo";

pub(crate) use application::generate_application;
pub(crate) use weather_data::generate_weather_data;
