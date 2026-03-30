use crate::client_common::tools::ToolSpec;

#[cfg(test)]
pub(crate) use nexal_code_mode::append_code_mode_sample;
#[cfg(test)]
pub(crate) use nexal_code_mode::render_json_schema_to_typescript;

#[cfg(test)]
#[path = "code_mode_description_tests.rs"]
mod code_mode_description_tests;

pub(crate) fn augment_tool_spec_for_code_mode(spec: ToolSpec, code_mode_enabled: bool) -> ToolSpec {
    if !code_mode_enabled {
        return spec;
    }

    match spec {
        ToolSpec::Function(mut tool) => {
            let input_type = serde_json::to_value(&tool.parameters)
                .ok()
                .map(|schema| nexal_code_mode::render_json_schema_to_typescript(&schema))
                .unwrap_or_else(|| "unknown".to_string());
            let output_type = tool
                .output_schema
                .as_ref()
                .map(nexal_code_mode::render_json_schema_to_typescript)
                .unwrap_or_else(|| "unknown".to_string());
            tool.description = nexal_code_mode::append_code_mode_sample(
                &tool.description,
                &tool.name,
                "args",
                input_type,
                output_type,
            );
            ToolSpec::Function(tool)
        }
        ToolSpec::Freeform(mut tool) => {
            if tool.name != nexal_code_mode::PUBLIC_TOOL_NAME {
                tool.description = nexal_code_mode::append_code_mode_sample(
                    &tool.description,
                    &tool.name,
                    "input",
                    "string".to_string(),
                    "unknown".to_string(),
                );
            }
            ToolSpec::Freeform(tool)
        }
        other => other,
    }
}
