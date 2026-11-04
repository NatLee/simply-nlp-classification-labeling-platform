import { BrowserRouter, Route, Routes } from "react-router";
import { Layout } from "./components/Layout";
import { SessionProvider } from "./lib/session";
import { AnnotatorsPage } from "./pages/Annotators";
import { DataPage } from "./pages/Data";
import { ItemsPage } from "./pages/Items";
import { LabelPage } from "./pages/Label";
import { ProjectLayout } from "./pages/ProjectLayout";
import { ProjectsPage } from "./pages/Projects";
import { SettingsPage } from "./pages/Settings";
import { StatsPage } from "./pages/Stats";

export function AppRoutes() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<ProjectsPage />} />
        <Route path="annotators" element={<AnnotatorsPage />} />
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
