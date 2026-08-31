import type { AppState } from './model';

type PatchInstruction<Value> =
  | Value
  | ((current: Value) => Value)
  | (Value extends readonly unknown[]
      ? never
      : Value extends object
        ? { readonly [Key in keyof Value]?: PatchInstruction<Value[Key]> }
        : never);

/** Immutable Mergerino-style object patch for the application state tree. */
export type AppPatch = {
  readonly [Slice in keyof AppState]?: PatchInstruction<AppState[Slice]>;
};

/** Function accepted by actions and event producers to enqueue a patch. */
export type AppUpdate = (patch: AppPatch) => void;
