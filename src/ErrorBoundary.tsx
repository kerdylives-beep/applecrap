import { Component, type ReactNode } from 'react'

type Props = {
  children: ReactNode
}

type State = {
  error: Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = {
    error: null,
  }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  componentDidCatch(error: Error, errorInfo: { componentStack: string }) {
    console.error('ErrorBoundary caught renderer error', error, errorInfo)
  }

  render() {
    if (this.state.error) {
      return (
        <main
          style={{
            minHeight: '100vh',
            padding: 32,
            display: 'grid',
            placeItems: 'center',
            color: '#edf5ff',
          }}
        >
          <section
            style={{
              maxWidth: 760,
              width: '100%',
              padding: 28,
              borderRadius: 22,
              border: '1px solid rgba(120, 152, 181, 0.2)',
              background: 'linear-gradient(180deg, rgba(21, 31, 44, 0.96), rgba(11, 18, 28, 0.98))',
            }}
          >
            <p style={{ marginTop: 0, marginBottom: 10, letterSpacing: '0.18em', textTransform: 'uppercase', color: '#ffb74d' }}>
              AppleCrap Alpha
            </p>
            <h1 style={{ marginTop: 0 }}>Renderer crashed</h1>
            <p>The React UI hit an error after loading.</p>
            <pre style={{ whiteSpace: 'pre-wrap', color: 'rgba(223, 234, 247, 0.78)' }}>
              {this.state.error.stack || this.state.error.message}
            </pre>
          </section>
        </main>
      )
    }

    return this.props.children
  }
}
