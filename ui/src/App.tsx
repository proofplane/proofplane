import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Route, Routes } from "react-router-dom";
import { AppRoute } from "./routes/AppRoute";
import { HomeRoute } from "./routes/HomeRoute";
import { NotFoundRoute } from "./routes/NotFoundRoute";

const queryClient = new QueryClient();

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Routes>
        <Route path="/" element={<HomeRoute />} />
        <Route path="/app" element={<AppRoute />} />
        <Route path="*" element={<NotFoundRoute />} />
      </Routes>
    </QueryClientProvider>
  );
}
