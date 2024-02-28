use std::collections::HashMap;

use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;

/// Returns a map from Sierra statement indexes to Cairo function names.
pub fn get_sierra_to_cairo_fn_names_map(
    sierra_program_with_debug: &SierraProgramWithDebug,
    compiler_db: &RootDatabase,
) -> HashMap<usize, String> {
    let statements_functions_map = sierra_program_with_debug
        .debug_info
        .statements_locations
        .get_statements_functions_map(compiler_db);
    let mut sierra_to_cairo_fn_names_map = HashMap::new();

    for (statement_idx, fn_name) in statements_functions_map.iter_sorted() {
        sierra_to_cairo_fn_names_map.insert(statement_idx.0, fn_name.to_string());
    }

    sierra_to_cairo_fn_names_map
}
