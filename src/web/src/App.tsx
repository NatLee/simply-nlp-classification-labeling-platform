import { BrowserRouter, Route, Routes } from "react-router";
import { Layout } from "./components/Layout";
import { Spinner } from "./components/ui";
import { SessionProvider, useSession } from "./lib/session";
import { AccountsPage } from "./pages/Accounts";
import { AnnotatorsPage } from "./pages/Annotators";
import { DataPage } from "./pages/Data";
import { ItemsPage } from "./pages/Items";
import { LabelPage } from "./pages/Label";
import { LoginPage } from "./pages/Login";
import { ProjectLayout } from "./pages/ProjectLayout";
import { ProjectsPage } from "./pages/Projects";
import { SettingsPage } from "./pages/Settings";
import { StatsPage } from "./pages/Stats";

export function AppRoutes() {
  const { auth } = useSession();
  if (auth === null) {
    return (
      <div className="grid min-h-dvh place-items-center text-zinc-400">
        <Spinner className="size-6" />
      </div>
    );
  }
  if (auth.enabled && !auth.user) return <LoginPage />;
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<ProjectsPage />} />
        <Route path="annotators" element={<AnnotatorsPage />} />
        <Route path="accounts" element={<AccountsPage />} />
        <Route path="p/:projectId" element={<ProjectLayout />}>
          <Route index element={<LabelPage />} />
          <Route path="items" element={<ItemsPage />} />
          <Route path="data" element={<DataPage />} />
          <Route path="stats" element={<StatsPage />} />
          <Route path="settings" element={<SettingsPage />} />
        </Route>
        <Route path="*" element={<ProjectsPage />} />
      </Route>
    </Routes>
  );
}

export function App() {
  return (
    <SessionProvider>
      <BrowserRouter>
        <AppRoutes />
      </BrowserRouter>
    </SessionProvider>
  );
}
