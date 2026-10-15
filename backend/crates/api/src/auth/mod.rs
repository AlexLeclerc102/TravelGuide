//! Authentification : mots de passe, jetons, sessions et extracteur `CurrentUser`.

pub mod extractor;
pub mod password;
pub mod session;
pub mod token;

pub use extractor::CurrentUser;
