// SPDX-License-Identifier: MPL-2.0
pub mod app;
pub mod db;
pub mod profiles;
pub mod protocol;
pub mod query;
pub mod results;
pub mod views;

pub type Result<T> = std::result::Result<T, String>;
pub const HOST_RANGE: &str = ">=0.3.0, <0.4.0";
pub const CAPABILITIES: &[&str] = &[
    "views",
    "interaction",
    "documents",
    "text",
    "selections",
    "jobs",
    "settings",
    "state",
    "activity",
];

pub mod browse;
pub mod documents;
pub mod inspection;
pub mod paths;
