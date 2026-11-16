// Minimal types for the parts of libheif-js we use (the package ships Emscripten-level types only).
declare module 'libheif-js/libheif-wasm/libheif-bundle.mjs' {
  export interface HeifImage {
    get_width(): number;
    get_height(): number;
    is_primary(): boolean;
    display(
      target: { data: Uint8ClampedArray<ArrayBuffer>; width: number; height: number },
      done: (result: { data: Uint8ClampedArray<ArrayBuffer>; width: number; height: number } | null) => void,
    ): void;
    free(): void;
  }
  export interface LibHeif {
    HeifDecoder: new () => { decode(bytes: Uint8Array): HeifImage[] };
  }
  const factory: () => LibHeif;
  export default factory;
}
