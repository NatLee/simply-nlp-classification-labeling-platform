import { FolderX } from "lucide-react";
import { Link, Outlet, useOutletContext, useParams } from "react-router";
import { api } from "../api";
import { EmptyState, Spinner } from "../components/ui";
import { useAsync } from "../lib/hooks";
import type { Project } from "../types";

type Ctx = { project: Project; reloadProject: () => Promise<void>; setProject: (p: Project) => void };

export function useProject() {
  return useOutletContext<Ctx>();
}

/** /p/:projectId 底下的頁面共用：先載入專案，再交給各頁。 */
export function ProjectLayout() {
  const projectId = Number(useParams().projectId);
  const { data: project, error, loading, reload, setData } = useAsync(() => api.project(projectId), [projectId]);
  if (error) {
    return (
      <EmptyState icon={<FolderX className="size-6" />} title="找不到這個專案">
        <Link className="btn btn-secondary mt-4" to="/">
          回到所有專案
        </Link>
      </EmptyState>
    );
  }
  if (loading && !project) {
    return (
      <div className="grid h-64 place-items-center text-zinc-400">
        <Spinner className="size-6" />
      </div>
    );
  }
  if (!project) return null;
  return <Outlet context={{ project, reloadProject: reload, setProject: setData } satisfies Ctx} />;
}
