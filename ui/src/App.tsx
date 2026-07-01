import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { Route, Routes } from "react-router-dom";
import { ProofplaneAuthProvider } from "./auth/AuthProvider";
import { AppRoute } from "./routes/AppRoute";
import { AuthCallbackRoute } from "./routes/AuthCallbackRoute";
import { DocsRoute } from "./routes/DocsRoute";
import { HomeRoute } from "./routes/HomeRoute";
import { NotFoundRoute } from "./routes/NotFoundRoute";
import { PricingRoute } from "./routes/PricingRoute";
import { McpAuthorizeRoute } from "./routes/McpAuthorizeRoute";

export function App() {
  const [queryClient] = useState(() => new QueryClient());

  return (
    <QueryClientProvider client={queryClient}>
      <ProofplaneAuthProvider>
        <Routes>
          <Route path="/" element={<HomeRoute />} />
          <Route path="/auth/callback" element={<AuthCallbackRoute />} />
          <Route path="/docs" element={<DocsRoute />} />
          <Route path="/pricing" element={<PricingRoute />} />
          <Route path="/connect/mcp/authorize" element={<McpAuthorizeRoute />} />
          <Route path="/app/*" element={<AppRoute />} />
          <Route path="*" element={<NotFoundRoute />} />
        </Routes>
      </ProofplaneAuthProvider>
    </QueryClientProvider>
  );
}
