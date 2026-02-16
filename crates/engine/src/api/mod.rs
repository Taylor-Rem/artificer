pub mod routes;
pub mod handlers;
pub mod server;
pub mod types;
mod middleware;

pub use server::start_server;