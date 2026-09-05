use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use spotlight_core::{
    GenerationClock, Icon, SearchQuery,
    desktop_entry::{CatalogDiagnostics, DesktopApplication, DesktopCatalog},
    history::UsageSnapshot,
    providers::applications::ApplicationProvider,
};

fn catalog(size: usize) -> DesktopCatalog {
    let applications = (0..size)
        .map(|index| DesktopApplication {
            desktop_id: format!("org.example.Application{index}.desktop"),
            source_path: PathBuf::from(format!(
                "/usr/share/applications/org.example.Application{index}.desktop"
            )),
            name: match index % 5 {
                0 => format!("Terminal Workspace {index}"),
                1 => format!("Text Editor {index}"),
                2 => format!("File Browser {index}"),
                3 => format!("Visual Studio Tool {index}"),
                _ => format!("Utility Application {index}"),
            },
            generic_name: Some("Desktop Application".into()),
            comment: None,
            keywords: vec!["productivity".into(), "utility".into()],
            categories: vec!["Utility".into()],
            executable_name: Some(format!("application-{index}")),
            icon: Icon::default(),
            secondary_actions: vec![],
        })
        .collect();
    DesktopCatalog {
        applications,
        diagnostics: CatalogDiagnostics::default(),
    }
}

fn application_search(c: &mut Criterion) {
    let size = 2_000;
    let provider = ApplicationProvider::new(
        catalog(size),
        Arc::new(RwLock::new(UsageSnapshot::default())),
    );
    let clock = GenerationClock::new();
    let token = clock.next();
    let query = SearchQuery::new(token.generation(), "vst", 8);

    let mut group = c.benchmark_group("application_search");
    group.throughput(Throughput::Elements(size as u64));
    group.bench_function("2000_entries_top_8", |b| {
        b.iter(|| provider.search_at(&query, &token, std::time::SystemTime::now()))
    });
    group.finish();
    c.bench_function("calculator/percentage", |b| {
        b.iter(|| {
            spotlight_core::providers::calculator::evaluate(std::hint::black_box("15% of 850"))
        })
    });
}

criterion_group!(benches, application_search);
criterion_main!(benches);
