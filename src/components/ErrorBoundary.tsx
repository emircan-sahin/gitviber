import { Component, type ErrorInfo, type ReactNode } from "react";

type Caught = { error: unknown; componentStack: string };

/** Shows `fallback` in place of a subtree that threw while rendering. main.tsx logs the error. */
export class ErrorBoundary extends Component<{ children: ReactNode; fallback: (error: unknown, componentStack: string) => ReactNode }, { caught: Caught | null }> {
  state = { caught: null as Caught | null };
  static getDerivedStateFromError(error: unknown) {
    return { caught: { error, componentStack: "" } };
  }
  componentDidCatch(error: unknown, info: ErrorInfo) {
    this.setState({ caught: { error, componentStack: info.componentStack ?? "" } });
  }
  render() {
    const { caught } = this.state;
    return caught ? this.props.fallback(caught.error, caught.componentStack) : this.props.children;
  }
}
