use std::io::{self, Read, Write};

use toml_edit::{Document, Item, Table, Value};

use crate::Version;

pub fn update_cargo_toml<In: Read, Out: Write>(
    input: io::BufReader<In>,
    mut output: Out,
    updates: &[(&str, &Version)],
    override_repository: &str,
) -> io::Result<()> {
    fn adjust_wgpu_deps(
        path: &str,
        dependencies: &mut Item,
        updates: &[(&str, &Version)],
        override_repository: &str,
    ) {
        let dependencies = dependencies
            .as_table_like_mut()
            .unwrap_or_else(|| panic!("`{path}` is not table-like :scream:"));

        for (dep_name, dep_source) in dependencies.iter_mut() {
            let dep_name = &*dep_name;
            let package = dep_source
                .as_table_like()
                .and_then(|tbl| tbl.get("package"))
                .and_then(|pkg| pkg.as_str());
            let crate_name = package.as_ref().unwrap_or(&dep_name);
            if let Some(version_to_apply) = updates
                .iter()
                .find_map(|(name, ver)| (name == crate_name).then_some(ver))
            {
                let mut new_table = match dep_source {
                    // A version string; we're just gonna replace the whole dang thing
                    Item::Value(Value::String(_)) => Table::new(),
                    Item::Table(tbl) => tbl.clone(),
                    Item::None | Item::ArrayOfTables(_) | Item::Value(_) => {
                        todo!("TODO: no idea what to do here yet")
                    }
                };
                {
                    // Remove any dep. fields relevant to source selection.
                    new_table.remove("path");
                    new_table.remove("branch");
                    new_table.remove("registry");
                    new_table.remove("version");
                }
                new_table.extend([
                    ("git", override_repository),
                    ("rev", &version_to_apply.git_hash),
                ]);
                *dep_source = Item::Table(new_table);
            }
        }
    }

    let mut document = io::read_to_string(input)
        .unwrap()
        .parse::<Document>()
        .expect("failed to read `Cargo.toml` file");

    for (key, val) in document.iter_mut() {
        if key == "dependencies" {
            adjust_wgpu_deps(&*key, val, updates, override_repository);
        } else if key.starts_with("target") {
            let target_table = val
                .as_table_like_mut()
                .expect("`target` key not table-like :scream:");
            for (key, val) in target_table.iter_mut() {
                if key.starts_with("cfg(") {
                    let cfg_table = val
                        .as_table_like_mut()
                        .expect("`target.{key}` key not table-like :scream:");
                    if let Some(deps) = cfg_table.get_mut("dependencies") {
                        adjust_wgpu_deps(
                            &format!("target.{key}.dependencies"),
                            deps,
                            updates,
                            override_repository,
                        );
                    }
                }
            }
        }
    }

    let document = document.to_string();
    output.write_all(document.as_bytes())
}
