# Yew Examples

This branch keeps only Lynx-specific examples in-tree.

- `react/`: a Yew rewrite of the ReactLynx starter example, using inline styles
  and `wasm32-wasip1` as the build target. The example is modeled as a Rust
  component state machine and uses the WAMR host timer callback ABI for frame
  ticks.
