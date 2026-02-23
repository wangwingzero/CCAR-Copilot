//! Supabase API 客户端模块
//!
//! 封装 Supabase 的 Auth API 和 Database API，
//! 提供认证、数据库操作等功能。
//!
//! # 模块结构
//! - `client`: HTTP 客户端基础设施
//! - `auth`: 用户认证服务
//! - `database`: 数据库操作服务

mod client;
mod auth;
mod database;

pub use client::{ProxyConfig, SupabaseClient, SupabaseConfig, SupabaseError};
pub use auth::{AuthService, AuthSession, AuthUser};
pub use database::{DatabaseService, QueryBuilder};
