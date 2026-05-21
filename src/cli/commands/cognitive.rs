//! `senclaw cognitive train` — offline GraphSAGE trainer.
//!
//! Walks the live cognitive graph (the same SQLite DB the daemon uses),
//! builds a training fixture, runs the SGD loop, and writes weights to
//! `~/.senclaw/cognitive/sage_<dim>.bin`. Designed to be safe to run
//! while the daemon is up — we only read.
//!
//! Outputs a short loss trace on stdout so you can eyeball convergence.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::db::Db;
use crate::memory::cognitive::{
    sage_train, GraphStore, SageModel, SageTrainConfig, SageTrainingFixture, SqliteGraphStore,
};
use crate::memory::embedding::{create_embedding_provider, EmbeddingProvider};

pub async fn train(
    epochs: usize,
    lr: f32,
    neg_per_pos: usize,
    max_nodes: Option<usize>,
) -> Result<()> {
    // Boot a minimal context — DB + embedding provider — without spinning
    // up the full daemon. The user can run this while `senclaw` is alive
    // because we don't take exclusive locks; SQLite WAL handles the rest.
    let mut cfg = Config::from_env();
    let gcp = cfg.paths.global_config_path.clone();
    cfg.apply_persisted_overrides(&gcp);

    let db = Arc::new(Db::open(&cfg).context("open senclaw db")?);
    let provider_box =
        create_embedding_provider(&cfg, Arc::clone(&db)).ok_or_else(|| {
            anyhow::anyhow!(
                "No embedding provider configured. Pick one in Settings → Embedding \
                 (or set SENCLAW_EMBEDDING_PROVIDER) before training."
            )
        })?;
    let provider: Arc<dyn EmbeddingProvider> = Arc::from(provider_box);
    let graph: Arc<dyn GraphStore> = Arc::new(SqliteGraphStore::new(Arc::clone(&db)));

    let dim = provider.dimensions() as usize;
    eprintln!(
        "[sage] training with provider={} dim={} epochs={} lr={} neg={}",
        provider.name(),
        dim,
        epochs,
        lr,
        neg_per_pos,
    );

    // Build the training fixture — this is where the network calls happen
    // (re-embedding every node's text). Could be slow on remote embedders;
    // we deliberately don't parallelise to stay polite to the API.
    eprintln!("[sage] building training fixture from cog_nodes/cog_edges…");
    let fixture = SageTrainingFixture::from_graph(&*graph, &*provider, max_nodes).await?;
    if fixture.embeddings.is_empty() {
        eprintln!("[sage] no nodes in the cognitive graph — nothing to train on.");
        return Ok(());
    }
    if fixture.edges.is_empty() {
        eprintln!(
            "[sage] {} node(s) but 0 edges — no positive samples for link \
             prediction. Add some triplets first (CogAdd full sentences).",
            fixture.embeddings.len()
        );
        return Ok(());
    }
    eprintln!(
        "[sage] fixture ready: {} nodes, {} positive edges",
        fixture.embeddings.len(),
        fixture.edges.len()
    );

    let mut model = SageModel::new_xavier(dim, /* seed */ 0x5A6E_DEAD);
    let cfg_train = SageTrainConfig {
        epochs,
        lr,
        neg_per_pos,
        max_nodes,
        ..Default::default()
    };
    let report = sage_train(&mut model, &fixture, &cfg_train);

    // Print the loss trace — every epoch on small runs, every Nth on big.
    let step = (epochs / 20).max(1);
    eprintln!("[sage] loss trace:");
    for (i, l) in report.losses.iter().enumerate() {
        if i % step == 0 || i + 1 == report.losses.len() {
            eprintln!("  epoch {i:>4}: loss = {l:.5}");
        }
    }
    eprintln!("[sage] final loss = {:.5}", report.final_loss);

    let path = SageModel::default_path(dim);
    model.save(&path).context("save weights")?;
    eprintln!("[sage] weights → {}", path.display());
    Ok(())
}
