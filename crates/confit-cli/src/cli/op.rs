use std::fmt::Display;

use crate::cli::ctx::Ctx;

pub trait Op {
    type Output: Display;
    type Error: std::error::Error + Send + Sync + 'static;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error>;
}

pub struct NoOutput;

impl Display for NoOutput {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

macro_rules! command_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident($type:ty)
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        pub enum $name {
            $(
                $(#[$vmeta])*
                $variant($type)
            ),*
        }

        pub enum OpOutput {
            $($variant(<$type as $crate::cli::op::Op>::Output)),*
        }

        pub enum OpError {
            $($variant(<$type as $crate::cli::op::Op>::Error)),*
        }

        impl std::fmt::Display for OpOutput {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(OpOutput::$variant(o) => o.fmt(f)),*
                }
            }
        }

        impl std::fmt::Display for OpError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(OpError::$variant(e) => e.fmt(f)),*
                }
            }
        }

        impl std::fmt::Debug for OpError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(OpError::$variant(e) => std::fmt::Debug::fmt(e, f)),*
                }
            }
        }

        impl std::error::Error for OpError {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    $(OpError::$variant(e) => e.source()),*
                }
            }
        }

        impl $crate::cli::op::Op for $name {
            type Output = OpOutput;
            type Error = OpError;

            fn run(&self, ctx: &$crate::cli::ctx::Ctx) -> Result<OpOutput, OpError> {
                match self {
                    $($name::$variant(cmd) => cmd
                        .run(ctx)
                        .map(OpOutput::$variant)
                        .map_err(OpError::$variant)),*
                }
            }
        }
    };
}

pub(crate) use command_enum;
