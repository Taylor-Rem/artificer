// crates/tools/src/macros.rs

#[macro_export]
macro_rules! register_toolbelt {
    (
        $toolbelt_type:ty {
            description: $toolbelt_desc:literal,
            location: $location:expr,
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

        pub static INSTANCE: Lazy<$toolbelt_type> = Lazy::new(<$toolbelt_type>::default);

        $(
            paste::paste! {
                pub fn [<$method _handler>](args: &serde_json::Value) -> anyhow::Result<String> {
                    INSTANCE.$method(args)
                }
            }
        )*

        paste::paste! {
            pub static TOOL_ENTRIES: &[(&str, $crate::schemas::ToolHandler)] = &[
                $((concat!(stringify!($toolbelt_type), "::", $name), [<$method _handler>])),*
            ];
        }

        pub static TOOL_SCHEMAS: Lazy<Vec<$crate::schemas::ToolSchema>> = Lazy::new(|| vec![
            $(
                $crate::schemas::ToolSchema {
                    name: concat!(stringify!($toolbelt_type), "::", $name),
                    description: $desc,
                    location: $location,
                    parameters: vec![
                        $(
                            $crate::schemas::ParameterSchema {
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