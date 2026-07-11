import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/** Render-crash fallback; reload re-attaches to the Rust-side scan session. */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="flex h-full items-center justify-center">
        <div className="max-w-lg rounded-lg border border-edge bg-panel p-6">
          <h1 className="text-sm font-medium text-ink">
            mathom hit an unexpected error
          </h1>
          <pre className="mt-3 overflow-auto text-xs whitespace-pre-wrap text-danger-ink select-text">
            {String(this.state.error)}
          </pre>
          <button
            onClick={() => location.reload()}
            className="mt-4 h-8 rounded-md bg-accent px-4 text-[13px] font-medium text-white hover:bg-accent-hover"
          >
            Reload
          </button>
        </div>
      </div>
    );
  }
}
