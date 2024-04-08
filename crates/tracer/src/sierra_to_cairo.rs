use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;
use cairo_lang_utils::Upcast;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct SierraToCairoDebugInfo {
    pub sierra_statements_to_cairo_info: HashMap<usize, SierraStatementToCairoDebugInfo>,
}

/// Human readable position inside a file, in lines and characters.
#[derive(Debug, Serialize, Clone)]
pub struct TextPosition {
    /// Line index, 0 based.
    pub line: usize,
    /// Character index inside the line, 0 based.
    pub col: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct Location {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug, Serialize)]
pub struct SierraStatementToCairoDebugInfo {
    pub fn_name: Option<String>,
    pub cairo_location: Option<Location>,
    pub cairo_locations: Vec<Location>,
}

/// Returns a map from Sierra statement indexes to Cairo function names.
pub fn get_sierra_to_cairo_debug_info(
    sierra_program_with_debug: &SierraProgramWithDebug,
    compiler_db: &RootDatabase,
) -> SierraToCairoDebugInfo {
    let statements_functions_map = sierra_program_with_debug
        .debug_info
        .statements_locations
        .get_statements_functions_map_for_tests(compiler_db);

    let mut sierra_statements_to_cairo_info: HashMap<usize, SierraStatementToCairoDebugInfo> =
        HashMap::new();

    for (statement_idx, locations) in sierra_program_with_debug
        .debug_info
        .statements_locations
        .locations
        .iter_sorted()
    {
        let mut cairo_locations: Vec<Location> = Vec::new();
        for location in locations {
            let syntax_node = location.syntax_node(compiler_db.upcast());
            let file_id = syntax_node.stable_ptr().file_id(compiler_db.upcast());
            let file_name = file_id.file_name(compiler_db.upcast());
            let syntax_node_location_span = syntax_node.span_without_trivia(compiler_db.upcast());
            let start = syntax_node_location_span
                .start
                .position_in_file(compiler_db.upcast(), file_id)
                .map(|s| TextPosition {
                    line: s.line,
                    col: s.col,
                });

            let end = syntax_node_location_span
                .end
                .position_in_file(compiler_db.upcast(), file_id)
                .map(|e| TextPosition {
                    line: e.line,
                    col: e.col,
                });

            let cairo_location = if file_name != "main.cairo" {
                None
            } else {
                start.zip(end).map(|(start, end)| Location { start, end })
            };
            if cairo_location.is_some() {
                cairo_locations.push(cairo_location.unwrap());
            }
        }
        sierra_statements_to_cairo_info.insert(
            statement_idx.0,
            SierraStatementToCairoDebugInfo {
                fn_name: statements_functions_map.get(statement_idx).cloned(),
                cairo_location: cairo_locations.first().cloned(),
                cairo_locations,
            },
        );
    }

    SierraToCairoDebugInfo {
        sierra_statements_to_cairo_info,
    }
}
