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

pub trait ToolBelt {
    fn description(&self) -> &'static str;
    fn use_tool(&self, name: &str, args: &Value) -> Result<String>;
    fn list_tools(&self) -> Vec<&'static str>;
    fn get_tool_schemas(&self) -> Vec<ToolSchema>;
}

pub struct ToolChest(pub fn() -> Box<dyn ToolBelt + Send + Sync>);

inventory::collect!(ToolChest);

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
        use phf::phf_map;

        type ToolHandler = fn(&$toolbelt_type, &serde_json::Value) -> anyhow::Result<String>;

        static TOOL_HANDLERS: phf::Map<&'static str, ToolHandler> = phf_map! {
            $($name => $toolbelt_type::$method),*
        };

        impl ToolBelt for $toolbelt_type {
            fn description(&self) -> &'static str {
                $toolbelt_desc
            }

            fn use_tool(&self, name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
                match TOOL_HANDLERS.get(name) {
                    Some(handler) => handler(self, args),
                    None => Err(anyhow::anyhow!("Tool '{}' not found in toolbelt", name)),
                }
            }

            fn list_tools(&self) -> Vec<&'static str> {
                vec![$($name),*]
            }

            fn get_tool_schemas(&self) -> Vec<ToolSchema> {
                vec![
                    $(
                        ToolSchema {
                            name: $name,
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
                ]
            }
        }
    };
}