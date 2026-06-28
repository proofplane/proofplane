export function AuthLoading() {
  return (
    <div className="auth-loading" role="status" aria-live="polite">
      <span className="loading-spinner" aria-hidden="true" />
      <span className="sr-only">Finishing sign in</span>
    </div>
  );
}
