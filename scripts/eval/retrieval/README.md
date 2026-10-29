# Retrieval check for chunk sizes

Hand-written questions (`queries.json`, each with the document and a phrase the answer chunk must
contain) against chunks of the Leiden corpus, embedded with multilingual-e5-small (ONNX).

```bash
PROCYON_DUMP_DIR=~/Downloads/corpus PROCYON_CHUNK_OUT=/tmp/chunks \
  cargo test --release -p fm-semantic-docling --test corpus_chunk_stats -- --ignored dump_chunks
uv run retrieve.py --chunks /tmp/chunks --model target/semantic-model-cache/intfloat--multilingual-e5-small/<rev>
```

Reports hit@1/3/5 and MRR per candidate maximum (estimated tokens). Chunks over the model's 512
tokens are truncated and counted: those rows only show what truncation costs; they are not
candidates for e5. Only 33 questions: differences of a few points are noise. A larger-context
model (EmbeddingGemma) needs its own runner.

## EmbeddingGemma

`crates/fm-semantic-worker/examples/evaluate_gemma_retrieval.rs` runs the same questions and chunk
dumps through the native EmbeddingGemma 2 text runner (about 5 minutes per size on CPU; the pool
holds only documents that have questions, so numbers differ from `retrieve.py`):

```bash
cargo build --release -p fm-semantic-worker --features gemma-probe --example evaluate_gemma_retrieval
GEMMA_MAXIMA=512,1000 target/release/examples/evaluate_gemma_retrieval \
  target/semantic-model-cache/google--embeddinggemma-2/<rev> /tmp/chunks scripts/eval/retrieval/queries.json
```
