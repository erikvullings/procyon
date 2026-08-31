declare module 'mergerino' {
  const merge: (target: unknown, ...patches: readonly unknown[]) => unknown;
  export default merge;
}
