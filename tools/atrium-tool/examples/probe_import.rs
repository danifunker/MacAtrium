//! Manual probe: run the full import against real `.mar` captures and show the
//! records it writes.
//!
//! `cargo run --example probe_import -- <rb-cli> <stage> <dataset> <archive>...`
//!
//! Exists because the interesting failure modes (which file is the `APPL`, which
//! names the host filesystem mangles, whether the record can be sourced at build
//! time) only show up against real captures.

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let rb_bin = args.next().expect("usage: probe_import <rb-cli> <stage> <dataset> <archive>...");
    let stage = std::path::PathBuf::from(args.next().expect("stage dir"));
    let dataset = std::path::PathBuf::from(args.next().expect("dataset"));
    let archives: Vec<std::path::PathBuf> = args.map(std::path::PathBuf::from).collect();
    let rb = atrium::rbcli::RbCli::new(&rb_bin);

    for a in &archives {
        println!("############ {}", a.display());
        match atrium::import::stage_archive(&rb, a, &stage) {
            Ok(s) => {
                println!("  id {}  name {:?}  folder {:?}", s.id, s.name, s.folder);
                println!("  app     {}", s.app_rel);
                println!("  files   {}", s.files.len());
                for (mac, host) in &s.renamed {
                    println!("  RENAMED {mac:?} -> {host:?}");
                }
            }
            Err(e) => println!("  FAILED: {e:#}"),
        }
    }

    println!("\n############ import::run -> {}", dataset.display());
    let report = atrium::import::run(&rb, &archives, &stage, &dataset, None, "/MacAtrium/Apps")?;
    for (id, name, n) in &report.imported {
        println!("  imported {id}  {name:?}  ({n} files)");
    }
    for (f, e) in &report.failed {
        println!("  FAILED {}: {e}", f.display());
    }
    println!("\n--- dataset ---");
    print!("{}", std::fs::read_to_string(&dataset).unwrap_or_default());

    println!("--- selection sees them as local (no donor needed) ---");
    let plan = atrium::selection::harvest_plan(
        &dataset,
        &atrium::config::Selection::All,
        None,
        &atrium::donors::Registry::default(),
        None,
    )?;
    println!("  local      {:?}", plan.local.iter().map(|(i, _)| i).collect::<Vec<_>>());
    println!("  unresolved {:?}", plan.unresolved);
    Ok(())
}
