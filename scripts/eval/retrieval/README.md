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
