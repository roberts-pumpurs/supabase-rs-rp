use rp_supabase_client::{select, schema::Selection};
use rp_supabase_codegen_example::database::public::tables::orders;

fn main() {
    let selected = select!(orders::Row => { id });
    let mut rows = selected.decode("[{\"id\":1}]").unwrap();
    let row = rows.remove(0);
    let _ = row.missing;
}
