import { NavLink, Navigate, Route, Routes } from "react-router-dom";
import SchemaList from "./pages/SchemaList";
import SchemaForm from "./pages/SchemaForm";
import SchemaView from "./pages/SchemaView";
import BuildRun from "./pages/BuildRun";
import RunsList from "./pages/RunsList";

const navLinkClass = ({ isActive }: { isActive: boolean }) =>
  `font-semibold no-underline ${isActive ? "text-[var(--accent)]" : "text-[var(--fg)]"}`;

export default function App() {
  return (
    <div className="min-h-screen">
      <nav className="flex gap-6 border-b border-[var(--border)] bg-[var(--head)] px-6 py-3">
        <NavLink to="/schemas" className={navLinkClass}>
          Schemas
        </NavLink>
        <NavLink to="/runs/new" className={navLinkClass}>
          Build run
        </NavLink>
        <NavLink to="/runs" className={navLinkClass}>
          Runs
        </NavLink>
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
