"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useAuthStore } from "@/lib/store";
import { apiFetch, cn } from "@/lib/utils";

interface BillingAccount {
  id: string;
  org_id: string;
  plan: string;
  status: string;
  billing_email?: string;
  has_subscription: boolean;
  created_at: string;
}

interface DailyUsage {
  date: string;
  value: number;
}

interface MeterUsage {
  meter: "storage_gb" | "compute_hours" | "api_calls";
  total: number;
  daily: DailyUsage[];
}

interface UsageSummary {
  period_start: string;
  period_end: string;
  meters: MeterUsage[];
}

const METER_META: Record<
  string,
  { label: string; unit: string; color: string; description: string }
> = {
  storage_gb: {
    label: "Database Storage",
    unit: "GB",
    color: "bg-emerald-500",
    description: "Measured from live pg_database_size() samples",
  },
  compute_hours: {
    label: "Compute Hours",
    unit: "h",
    color: "bg-indigo-500",
    description: "Weighted running time of compute endpoints",
  },
  api_calls: {
    label: "API Calls",
    unit: "calls",
    color: "bg-amber-500",
    description: "REST requests served by the API gateway",
  },
};

export default function BillingPage() {
  const router = useRouter();
  const { isAuthenticated, user } = useAuthStore();
  const [account, setAccount] = useState<BillingAccount | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isAuthenticated) {
      router.push("/login");
      return;
    }
    loadData();
  }, [isAuthenticated, router]);

  const loadData = async () => {
    try {
      const [accountRes, usageRes] = await Promise.all([
        apiFetch<{ data: BillingAccount }>("/v1/billing/account"),
        apiFetch<{ data: UsageSummary }>("/v1/billing/usage"),
      ]);
      setAccount(accountRes.data);
      setUsage(usageRes.data);
    } catch (err: any) {
      setError(err.message || "Failed to load billing data");
    } finally {
      setLoading(false);
    }
  };

  const startCheckout = async () => {
    setBusy("checkout");
    try {
      const res = await apiFetch<{ data: { url: string } }>("/v1/billing/checkout", {
        method: "POST",
        body: JSON.stringify({
          success_url: `${window.location.origin}/billing?checkout=success`,
          cancel_url: `${window.location.origin}/billing?checkout=canceled`,
        }),
      });
      window.location.href = res.data.url;
    } catch (err: any) {
      alert(err.message || "Failed to start checkout");
      setBusy(null);
    }
  };

  const openPortal = async () => {
    setBusy("portal");
    try {
      const res = await apiFetch<{ data: { url: string } }>("/v1/billing/portal", {
        method: "POST",
        body: JSON.stringify({ return_url: window.location.origin + "/billing" }),
      });
      window.location.href = res.data.url;
    } catch (err: any) {
      alert(err.message || "Failed to open billing portal");
      setBusy(null);
    }
  };

  const cancelSubscription = async () => {
    if (!confirm("Cancel your subscription? It stays active until the end of the billing period.")) return;
    setBusy("cancel");
    try {
      const res = await apiFetch<{ data: BillingAccount }>("/v1/billing/cancel", {
        method: "POST",
      });
      setAccount(res.data);
    } catch (err: any) {
      alert(err.message || "Failed to cancel subscription");
    } finally {
      setBusy(null);
    }
  };

  const statusBadge = (status: string) => {
    const styles: Record<string, string> = {
      free: "bg-gray-100 text-gray-700",
      active: "bg-green-100 text-green-800",
      trialing: "bg-blue-100 text-blue-800",
      past_due: "bg-red-100 text-red-800",
      canceled: "bg-gray-100 text-gray-500",
      unpaid: "bg-red-100 text-red-800",
    };
    return (
      <span className={cn("inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium", styles[status] || "bg-gray-100 text-gray-700")}>
        {status}
      </span>
    );
  };

  const maxDaily = (meter: MeterUsage) =>
    Math.max(1, ...meter.daily.map((d) => d.value));

  const formatValue = (meter: string, value: number) => {
    if (meter === "api_calls") return Math.round(value).toLocaleString();
    return value.toFixed(2);
  };

  if (loading) {
    return (
      <main className="min-h-screen bg-gray-50 p-8">
        <div className="animate-pulse text-lg text-gray-400">Loading billing...</div>
      </main>
    );
  }

  return (
    <main className="min-h-screen bg-gray-50">
      {/* Header */}
      <header className="border-b bg-white">
        <div className="mx-auto flex max-w-7xl items-center justify-between px-8 py-4">
          <div className="flex items-center gap-3">
            <Link href="/projects" className="text-xl font-bold text-gray-900">
              Freebuff
            </Link>
            <span className="text-sm text-gray-400">/</span>
            <span className="text-sm text-gray-600">Billing</span>
          </div>
          <div className="flex items-center gap-4">
            <Link href="/projects" className="text-sm text-gray-500 hover:text-gray-700">
              Projects
            </Link>
            <span className="text-sm text-gray-600">{user?.email}</span>
            <button
              onClick={() => {
                useAuthStore.getState().logout();
                router.push("/login");
              }}
              className="text-sm text-gray-500 hover:text-gray-700"
            >
              Sign out
            </button>
          </div>
        </div>
      </header>

      <div className="mx-auto max-w-7xl px-8 py-8">
        {error && (
          <div className="mb-6 rounded-lg border border-red-200 bg-red-50 p-4 text-sm text-red-800">
            {error}
          </div>
        )}

        {/* Plan card */}
        <div className="rounded-lg border bg-white p-6 shadow-sm">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div>
              <div className="flex items-center gap-3">
                <h1 className="text-2xl font-bold text-gray-900">
                  {account?.plan === "free" ? "Free Plan" : `${account?.plan} Plan`}
                </h1>
                {account && statusBadge(account.status)}
              </div>
              <p className="mt-2 text-sm text-gray-600">
                {account?.has_subscription
                  ? "Billed through Stripe. Usage is metered per billing period."
                  : "Upgrade to a paid plan to unlock usage-based billing via Stripe."}
              </p>
              {account?.billing_email && (
                <p className="mt-1 text-xs text-gray-500">
                  Billing email: {account.billing_email}
                </p>
              )}
            </div>
            <div className="flex items-center gap-3">
              {account?.has_subscription ? (
                <>
                  <button
                    onClick={openPortal}
                    disabled={busy === "portal"}
                    className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
                  >
                    {busy === "portal" ? "Opening..." : "Manage billing"}
                  </button>
                  <button
                    onClick={cancelSubscription}
                    disabled={busy === "cancel"}
                    className="rounded-md border border-red-200 px-4 py-2 text-sm font-medium text-red-600 hover:bg-red-50 disabled:opacity-50"
                  >
                    {busy === "cancel" ? "Canceling..." : "Cancel subscription"}
                  </button>
                </>
              ) : (
                <button
                  onClick={startCheckout}
                  disabled={busy === "checkout"}
                  className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-700 disabled:opacity-50"
                >
                  {busy === "checkout" ? "Redirecting..." : "Upgrade"}
                </button>
              )}
            </div>
          </div>
        </div>

        {/* Usage meters */}
        <div className="mt-8">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold text-gray-900">Usage this period</h2>
            {usage && (
              <p className="text-xs text-gray-500">
                {new Date(usage.period_start).toLocaleDateString()} — {new Date(usage.period_end).toLocaleDateString()}
              </p>
            )}
          </div>

          {usage && usage.meters.length === 0 && (
            <div className="mt-4 rounded-lg border-2 border-dashed border-gray-300 p-10 text-center">
              <p className="text-sm text-gray-500">
                No usage recorded yet. It appears here within a minute of activity.
              </p>
            </div>
          )}

          <div className="mt-4 grid gap-6 md:grid-cols-3">
            {usage?.meters.map((meter) => {
              const meta = METER_META[meter.meter] || {
                label: meter.meter,
                unit: "",
                color: "bg-gray-500",
                description: "",
              };
              return (
                <div key={meter.meter} className="rounded-lg border bg-white p-6 shadow-sm">
                  <div className="flex items-start justify-between">
                    <div>
                      <h3 className="font-semibold text-gray-900">{meta.label}</h3>
                      <p className="mt-1 text-xs text-gray-500">{meta.description}</p>
                    </div>
                    <span className="text-right">
                      <span className="text-2xl font-bold text-gray-900">
                        {formatValue(meter.meter, meter.total)}
                      </span>
                      <span className="ml-1 text-sm text-gray-500">{meta.unit}</span>
                    </span>
                  </div>
                  <div className="mt-4 flex h-20 items-end gap-1">
                    {meter.daily.map((day) => (
                      <div
                        key={day.date}
                        title={`${day.date}: ${formatValue(meter.meter, day.value)} ${meta.unit}`}
                        className={cn("flex-1 rounded-t", meta.color)}
                        style={{ height: `${Math.max(4, (day.value / maxDaily(meter)) * 100)}%` }}
                      />
                    ))}
                    {meter.daily.length === 0 && (
                      <div className="flex h-full w-full items-center justify-center text-xs text-gray-400">
                        No data
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </main>
  );
}