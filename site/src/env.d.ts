interface PlausibleFunction {
  (...args: unknown[]): void;
  q?: IArguments[];
}

interface Window {
  plausible: PlausibleFunction;
}
