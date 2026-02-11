use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Vec<ParameterSchema>,
}

#[derive(Debug, Clone)]
pub struct ParameterSchema {
    pub name: &'static str,
    pub type_name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

/// Type alias for tool handler functions used in the registry
pub type ToolHandler = fn(&Value) -> Result<String>;

#[macro_export]
macro_rules! register_toolbelt {
    (
        $toolbelt_type:ty {
            description: $toolbelt_desc:literal,
            tools: {
                $(
                    $name:literal => $method:ident {
                        description: $desc:literal,
                        params: [$($param_name:literal: $param_type:literal => $param_desc:literal),* $(,)?]
                    }
                ),* $(,)?
            }
        }
    ) => {
        use once_cell::sync::Lazy;

        // Lazy singleton instance
        pub static INSTANCE: Lazy<$toolbelt_type> = Lazy::new(<$toolbelt_type>::default);

        // Generate wrapper functions that call the singleton
        $(
            paste::paste! {
                pub fn [<$method _handler>](args: &serde_json::Value) -> anyhow::Result<String> {
                    INSTANCE.$method(args)
                }
            }
        )*

        // Tool entries for registry (namespaced: "TypeName::tool_name")
        paste::paste! {
            pub static TOOL_ENTRIES: &[(&str, $crate::traits::ToolHandler)] = &[
                $((concat!(stringify!($toolbelt_type), "::", $name), [<$method _handler>])),*
            ];
        }

        // Tool schemas for LLM consumption
        pub static TOOL_SCHEMAS: Lazy<Vec<ToolSchema>> = Lazy::new(|| vec![
            $(
                ToolSchema {
                    name: concat!(stringify!($toolbelt_type), "::", $name),
                    description: $desc,
                    parameters: vec![
                        $(
                            ParameterSchema {
                                name: $param_name,
                                type_name: $param_type,
                                description: $param_desc,
                                required: true,
                            }
                        ),*
                    ],
                }
            ),*
        ]);
    };
}
