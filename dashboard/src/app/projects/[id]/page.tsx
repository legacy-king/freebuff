"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useAuthStore } from "@/lib/store";
import { apiFetch, cn, formatDate } from "@/lib/utils";

interface Project {
  id: string;
  name: string;
  slug: string;
  region: string;
  status: string;
  database_host?: string;
  database_port?: number;
  database_name?: string;
  created_at: string;
}

interface Branch {
  id: string;
  name: string;
  slug: string;
  status: string;
  is_default: boolean;
  created_at: string;
}

interface ApiKey {
  id: string;
  name: string;
  key_prefix: string;
  key_type: string;
  created_at: string;
  last_used_at?: string;
}

type Tab = "overview" | "branches" | "api-keys";

export default function ProjectPage() {
  const params = useParams();
  const router = useRouter();
  const { isAuthenticated } = useAuthStore();
  const projectId = params.id as string;

  const [project, setProject] = useState<Project | null>(null);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [tab, setTab] = useState<Tab>("overview");
  const [loading, setLoading] = useState(true);
  const [showNewBranch, setShowNewBranch] = useState(false);
  const [newBranchName, setNewBranchName] = useState("");
  const [showNewKey, setShowNewKey] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [newKeyType, setNewKeyType] = useState<"publishable" | "secret">("publishable");
  const [newKeyValue, setNewKeyValue] = useState<string | null>(null);

  useEffect(() => {
    if (!isAuthenticated) {
      router.push("/login");
      return;
    }
    loadData();
  }, [isAuthenticated, projectId, router]);

  const loadData = async () => {
    try {
      const [projectRes, branchesRes, keysRes] = await Promise.all([
        apiFetch<{ data: Project }>(`/v1/projects/${projectId}`),
        apiFetch<Branch[]>(`/v1/projects/${projectId}/branches`),
        apiFetch<ApiKey[]>(`/v1/projects/${projectId}/api-keys`),
      ]);
      setProject(projectRes.data);
      setBranches(branchesRes);
      setApiKeys(keysRes);
    } catch (err) {
      console.error("Failed to load project:", err);
    } finally {
      setLoading(false);
    }
  };

  const createBranch = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const response = await apiFetch<{ data: Branch }>(
        `/v1/projects/${projectId}/branches`,
        {
          method: "POST",
          body: JSON.stringify({ name: newBranchName }),
        }
      );
      setBranches([response.data, ...branches]);
      setShowNewBranch(false);
      setNewBranchName("");
    } catch (err: any) {
      alert(err.message);
    }
  };

  const createApiKey = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const response = await apiFetch<{ data: { id: string; name: string; key: string; key_prefix: string; key_type: string; created_at: string } }>(
        `/v1/projects/${projectId}/api-keys`,
        {
          method: "POST",
          body: JSON.stringify({ name: newKeyName, key_type: newKeyType }),
        }
      );
      setNewKeyValue(response.data.key);
      setApiKeys([{ ...response.data, key_prefix: response.data.key_prefix } as ApiKey, ...apiKeys]);
      setShowNewKey(false);
      setNewKeyName("");
    } catch (err: any) {
      alert(err.message);
    }
  };

  const deleteApiKey = async (keyId: string) => {
    if (!confirm("Delete this API key?")) return;
    try {
      await apiFetch(`/v1/projects/${projectId}/api-keys/${keyId}`, {
        method: "DELETE",
      });
      setApiKeys(apiKeys.filter((k) => k.id !== keyId));
    } catch (err: any) {
      alert(err.message);
    }
  };

  if (loading) {
    return (
      <main className="min-h-screen bg-gray-50 p-8">
        <div className="animate-pulse text-lg text-gray-400">Loading project...</div>
      </main>
    );
  }

  if (!project) {
    return (
      <main className="min-h-screen bg-gray-50 p-8">
        <div className="text-lg text-red-500">Project not found</div>
      </main>
    );
  }

  const connectionUri = project.database_host
    ? `postgresql://postgres:[password]@${project.database_host}:${project.database_port || 5432}/${project.database_name}`
    : "Not available yet";

  return (
    <main className="min-h-screen bg-gray-50">
      {/* Header */}
      <header className="border-b bg-white">
        <div className="mx-auto flex max-w-7xl items-center justify-between px-8 py-4">
          <div className="flex items-center gap-3">
            <Link href="/projects" className="text-sm text-gray-500 hover:text-gray-700">
              ← Projects
            </Link>
            <Link href="/billing" className="text-sm text-gray-500 hover:text-gray-700">
              Billing
            </Link>
            <h1 className="text-xl font-bold text-gray-900">{project.name}</h1>
            <span
              className={cn(
                "inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium",
                project.status === "active"
                  ? "bg-green-100 text-green-800"
                  : "bg-yellow-100 text-yellow-800"
              )}
            >
              {project.status}
            </span>
          </div>
        </div>
      </header>

      {/* Tabs */}
      <div className="border-b bg-white">
        <div className="mx-auto max-w-7xl px-8">
          <nav className="flex gap-6">
            {(["overview", "branches", "api-keys"] as Tab[]).map((t) => (
              <button
                key={t}
                onClick={() => setTab(t)}
                className={cn(
                  "border-b-2 py-3 text-sm font-medium transition-colors",
                  tab === t
                    ? "border-indigo-600 text-indigo-600"
                    : "border-transparent text-gray-500 hover:text-gray-700"
                )}
              >
                {t === "api-keys" ? "API Keys" : t.charAt(0).toUpperCase() + t.slice(1)}
              </button>
            ))}
          </nav>
        </div>
      </div>

      {/* Content */}
      <div className="mx-auto max-w-7xl px-8 py-8">
        {tab === "overview" && (
          <div className="space-y-6">
            {/* Connection Info */}
            <div className="rounded-lg border bg-white p-6 shadow-sm">
              <h2 className="text-lg font-semibold">Connection</h2>
              <div className="mt-4 space-y-3">
                <div>
                  <label className="text-xs font-medium text-gray-500">PostgreSQL URI</label>
                  <div className="mt-1 flex items-center gap-2">
                    <code className="flex-1 rounded bg-gray-100 px-3 py-2 text-sm text-gray-800">
                      {connectionUri}
                    </code>
                    <button
                      onClick={() => navigator.clipboard.writeText(connectionUri)}
                      className="rounded bg-gray-100 px-3 py-2 text-sm text-gray-600 hover:bg-gray-200"
                    >
                      Copy
                    </button>
                  </div>
                </div>
                <div className="grid grid-cols-3 gap-4">
                  <div>
                    <label className="text-xs font-medium text-gray-500">Host</label>
                    <p className="text-sm text-gray-900">{project.database_host || "—"}</p>
                  </div>
                  <div>
                    <label className="text-xs font-medium text-gray-500">Port</label>
                    <p className="text-sm text-gray-900">{project.database_port || "—"}</p>
                  </div>
                  <div>
                    <label className="text-xs font-medium text-gray-500">Database</label>
                    <p className="text-sm text-gray-900">{project.database_name || "—"}</p>
                  </div>
                </div>
              </div>
            </div>

            {/* Project Info */}
            <div className="rounded-lg border bg-white p-6 shadow-sm">
              <h2 className="text-lg font-semibold">Details</h2>
              <dl className="mt-4 grid grid-cols-2 gap-4">
                <div>
                  <dt className="text-xs font-medium text-gray-500">Project ID</dt>
                  <dd className="mt-1 font-mono text-sm text-gray-900">{project.id}</dd>
                </div>
                <div>
                  <dt className="text-xs font-medium text-gray-500">Region</dt>
                  <dd className="mt-1 text-sm text-gray-900">{project.region}</dd>
                </div>
                <div>
                  <dt className="text-xs font-medium text-gray-500">Created</dt>
                  <dd className="mt-1 text-sm text-gray-900">{formatDate(project.created_at)}</dd>
                </div>
                <div>
                  <dt className="text-xs font-medium text-gray-500">Slug</dt>
                  <dd className="mt-1 text-sm text-gray-900">{project.slug}</dd>
                </div>
              </dl>
            </div>
          </div>
        )}

        {tab === "branches" && (
          <div>
            <div className="flex items-center justify-between">
              <h2 className="text-lg font-semibold">Branches</h2>
              <button
                onClick={() => setShowNewBranch(true)}
                className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-700"
              >
                New Branch
              </button>
            </div>

            {showNewBranch && (
              <form onSubmit={createBranch} className="mt-4 rounded-lg border bg-white p-4 shadow-sm">
                <div className="flex gap-3">
                  <input
                    type="text"
                    value={newBranchName}
                    onChange={(e) => setNewBranchName(e.target.value)}
                    placeholder="Branch name"
                    className="flex-1 rounded-md border px-3 py-2 text-sm"
                    autoFocus
                  />
                  <button type="submit" className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white">
                    Create
                  </button>
                  <button type="button" onClick={() => setShowNewBranch(false)} className="rounded-md border px-4 py-2 text-sm">
                    Cancel
                  </button>
                </div>
              </form>
            )}

            <div className="mt-4 space-y-3">
              {branches.map((branch) => (
                <div key={branch.id} className="flex items-center justify-between rounded-lg border bg-white p-4 shadow-sm">
                  <div>
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-gray-900">{branch.name}</span>
                      {branch.is_default && (
                        <span className="rounded-full bg-blue-100 px-2 py-0.5 text-xs text-blue-800">
                          default
                        </span>
                      )}
                    </div>
                    <p className="text-sm text-gray-500">Created {formatDate(branch.created_at)}</p>
                  </div>
                  <span className={cn(
                    "rounded-full px-2.5 py-0.5 text-xs font-medium",
                    branch.status === "active" ? "bg-green-100 text-green-800" : "bg-gray-100 text-gray-600"
                  )}>
                    {branch.status}
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}

        {tab === "api-keys" && (
          <div>
            <div className="flex items-center justify-between">
              <h2 className="text-lg font-semibold">API Keys</h2>
              <button
                onClick={() => setShowNewKey(true)}
                className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-700"
              >
                Generate Key
              </button>
            </div>

            {newKeyValue && (
              <div className="mt-4 rounded-lg border border-green-200 bg-green-50 p-4">
                <p className="text-sm font-medium text-green-800">
                  ⚠️ Copy this key now — it won&apos;t be shown again:
                </p>
                <code className="mt-2 block rounded bg-white p-3 text-sm break-all">{newKeyValue}</code>
                <button
                  onClick={() => setNewKeyValue(null)}
                  className="mt-2 text-sm text-green-700 underline"
                >
                  I&apos;ve saved it
                </button>
              </div>
            )}

            {showNewKey && (
              <form onSubmit={createApiKey} className="mt-4 rounded-lg border bg-white p-4 shadow-sm">
                <div className="flex gap-3">
                  <input
                    type="text"
                    value={newKeyName}
                    onChange={(e) => setNewKeyName(e.target.value)}
                    placeholder="Key name"
                    className="flex-1 rounded-md border px-3 py-2 text-sm"
                    autoFocus
                  />
                  <select
                    value={newKeyType}
                    onChange={(e) => setNewKeyType(e.target.value as "publishable" | "secret")}
                    className="rounded-md border px-3 py-2 text-sm"
                  >
                    <option value="publishable">Publishable</option>
                    <option value="secret">Secret</option>
                  </select>
                  <button type="submit" className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white">
                    Generate
                  </button>
                  <button type="button" onClick={() => setShowNewKey(false)} className="rounded-md border px-4 py-2 text-sm">
                    Cancel
                  </button>
                </div>
              </form>
            )}

            <div className="mt-4 space-y-3">
              {apiKeys.map((key) => (
                <div key={key.id} className="flex items-center justify-between rounded-lg border bg-white p-4 shadow-sm">
                  <div>
                    <span className="font-medium text-gray-900">{key.name}</span>
                    <span className="ml-2 font-mono text-sm text-gray-500">{key.key_prefix}...</span>
                    <span className={cn(
                      "ml-2 rounded-full px-2 py-0.5 text-xs",
                      key.key_type === "secret" ? "bg-red-100 text-red-800" : "bg-blue-100 text-blue-800"
                    )}>
                      {key.key_type}
                    </span>
                  </div>
                  <button
                    onClick={() => deleteApiKey(key.id)}
                    className="text-sm text-red-500 hover:text-red-700"
                  >
                    Delete
                  </button>
                </div>
              ))}
              {apiKeys.length === 0 && (
                <p className="text-sm text-gray-500">No API keys yet. Create one to start using the API.</p>
              )}
            </div>
          </div>
        )}
      </div>
    </main>
  );
}
