use getset::Getters;
use serde::Deserialize;

#[derive(Clone, Deserialize, Getters)]
#[getset(get = "pub")]
pub struct Response {
    error: bool,
    response: Vec<EventIndex>,
}

#[derive(Clone, Deserialize, Getters)]
#[getset(get = "pub")]
pub struct EventIndex {
    id: u64,
    name: String,
    departure: Location,
    start_at: String,
    banner: Option<String>,
    description: String,
    url: String,
}

#[derive(Clone, Deserialize, Getters)]
#[getset(get = "pub")]
pub struct Location {
    city: String,
}
