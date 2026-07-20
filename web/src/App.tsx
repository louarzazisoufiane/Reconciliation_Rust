import { NavLink, Navigate, Route, Routes } from "react-router-dom";
import SchemaList from "./pages/SchemaList";
import SchemaForm from "./pages/SchemaForm";
import SchemaView from "./pages/SchemaView";
import BuildRun from "./pages/BuildRun";
import RunsList from "./pages/RunsList";

const navLinkClass = ({ isActive }: { isActive: boolean }) =>
  `rounded-lg px-3 py-1.5 text-sm font-semibold no-underline transition-colors ${
    isActive
      ? "bg-[var(--accent-soft)] text-[var(--accent)]"
      : "text-[var(--muted)] hover:text-[var(--fg)]"
  }`;

export default function App() {
  return (
    <div className="min-h-screen">
      <nav className="sticky top-0 z-10 border-b border-[var(--border)] bg-[var(--head)]/85 backdrop-blur-md">
        <div className="mx-auto flex max-w-5xl items-center gap-6 px-6 py-3">
          <span className="flex items-center gap-2 font-semibold tracking-tight">
            <span className="grid h-7 w-7 place-items-center rounded-lg bg-[var(--accent)] text-white shadow-[var(--shadow-sm)]">
              ⇄
            </span>
            <span className="font-display text-[var(--fg)]">Reconcile</span>
          </span>
          <div className="flex gap-1">
            <NavLink to="/schemas" className={navLinkClass}>
              Schemas
            </NavLink>
            <NavLink to="/runs/new" className={navLinkClass}>
              Build run
            </NavLink>
            <NavLink to="/runs" className={navLinkClass}>
              Runs
            </NavLink>
          </div>
        </div>
      </nav>
      <main className="mx-auto max-w-5xl px-6 py-8">
        <Routes>
          <Route path="/" element={<Navigate to="/schemas" replace />} />
          <Route path="/schemas" element={<SchemaList />} />
          <Route path="/schemas/new" element={<SchemaForm mode="create" />} />
          <Route path="/schemas/:name/edit" element={<SchemaForm mode="edit" />} />
          <Route path="/schemas/:name" element={<SchemaView />} />
          <Route path="/runs/new" element={<BuildRun />} />
          <Route path="/runs" element={<RunsList />} />
        </Routes>
      </main>
    </div>
  );
}
