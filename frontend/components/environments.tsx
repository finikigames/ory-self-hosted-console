"use client";

import { ChevronDown, Plus, Trash2 } from "lucide-react";
import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { api, ApiError } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

// Contours stored on the logged-in admin account. "Local" is the built-in
// deployment (process env) and is not a row. Selecting a row retargets this
// browser session's Kratos/Hydra/Keto admin calls.

export type Environment = {
  id: string;
  name: string;
  kratos_admin_url: string;
  hydra_admin_url: string;
  hydra_public_url: string;
  keto_read_url: string | null;
  keto_write_url: string | null;
  keto_opl_url: string | null;
};

type EnvironmentList = {
  active_id: string | null;
  environments: Environment[];
};

type Draft = {
  id: string | null;
  name: string;
  kratos_admin_url: string;
  hydra_admin_url: string;
  hydra_public_url: string;
  keto_read_url: string;
  keto_write_url: string;
  keto_opl_url: string;
};

const EMPTY_DRAFT: Draft = {
  id: null,
  name: "",
  kratos_admin_url: "",
  hydra_admin_url: "",
  hydra_public_url: "",
  keto_read_url: "",
  keto_write_url: "",
  keto_opl_url: "",
};

type EnvironmentContextValue = {
  data: EnvironmentList | null;
  error: string | null;
  reload: () => Promise<void>;
};

const EnvironmentContext = createContext<EnvironmentContextValue | null>(null);

function useEnvironments(): EnvironmentContextValue {
  const value = useContext(EnvironmentContext);
  if (!value) {
    throw new Error("EnvironmentProvider is missing");
  }
  return value;
}

export function EnvironmentProvider({ children }: { children: ReactNode }) {
  const [data, setData] = useState<EnvironmentList | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function reload() {
    try {
      const next = await api<EnvironmentList>("/api/console/environments");
      setData(next);
      setError(null);
    } catch {
      setError("Could not load contours.");
    }
  }

  useEffect(() => {
    void reload();
  }, []);

  return (
    <EnvironmentContext.Provider value={{ data, error, reload }}>
      {children}
    </EnvironmentContext.Provider>
  );
}

function activeEnvironment(data: EnvironmentList | null): Environment | null {
  if (!data?.active_id) return null;
  return data.environments.find((env) => env.id === data.active_id) ?? null;
}

function isProdName(name: string): boolean {
  return /\bprod(uction)?\b/i.test(name);
}

export function EnvironmentSwitcher() {
  const { data, error, reload } = useEnvironments();
  const [busy, setBusy] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  const active = activeEnvironment(data);
  const label = active?.name ?? "Local";

  async function select(id: string) {
    const next = id === "local" ? null : id;
    if ((data?.active_id ?? null) === next) return;
    setBusy(true);
    try {
      await api("/api/console/environments/active", {
        method: "PUT",
        body: JSON.stringify({ id: next }),
      });
      window.location.reload();
    } catch {
      setBusy(false);
      await reload();
    }
  }

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant={active && isProdName(active.name) ? "destructive" : "outline"}
            size="sm"
            className="gap-2"
            disabled={busy || !data}
            aria-label="Active contour"
          >
            <span className="max-w-[10rem] truncate">
              {error ? "Contour" : label}
            </span>
            <ChevronDown className="size-4 opacity-60" aria-hidden />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-64">
          <DropdownMenuLabel className="text-muted-foreground text-xs font-normal">
            Contour
          </DropdownMenuLabel>
          <DropdownMenuRadioGroup
            value={data?.active_id ?? "local"}
            onValueChange={(value) => {
              void select(value);
            }}
          >
            <DropdownMenuRadioItem value="local">Local</DropdownMenuRadioItem>
            {data?.environments.map((env) => (
              <DropdownMenuRadioItem key={env.id} value={env.id}>
                <span className="truncate">{env.name}</span>
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            onSelect={(event) => {
              event.preventDefault();
              setManageOpen(true);
            }}
          >
            Manage contours…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <ManageDialog open={manageOpen} onOpenChange={setManageOpen} />
    </>
  );
}

export function EnvironmentBanner() {
  const { data } = useEnvironments();
  const active = activeEnvironment(data);
  if (!active) return null;
  const keto = active.keto_read_url
    ? "Keto follows this contour too."
    : "Keto stays on Local until you set its URLs.";
  return (
    <div className="border-b bg-amber-500/10 px-4 py-2 text-sm">
      <span className="font-medium">{active.name}</span>
      {" — identities, OAuth2 clients, and permissions use this contour. "}
      {keto}{" "}
      Configuration files, restarts, and observability stay on this console's
      local deployment.
    </div>
  );
}

function ManageDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { data, reload } = useEnvironments();
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  function edit(env: Environment) {
    setFormError(null);
    setDraft({
      id: env.id,
      name: env.name,
      kratos_admin_url: env.kratos_admin_url,
      hydra_admin_url: env.hydra_admin_url,
      hydra_public_url: env.hydra_public_url,
      keto_read_url: env.keto_read_url ?? "",
      keto_write_url: env.keto_write_url ?? "",
      keto_opl_url: env.keto_opl_url ?? "",
    });
  }

  async function save() {
    setBusy(true);
    setFormError(null);
    const body = {
      name: draft.name,
      kratos_admin_url: draft.kratos_admin_url,
      hydra_admin_url: draft.hydra_admin_url,
      hydra_public_url: draft.hydra_public_url,
      keto_read_url: draft.keto_read_url,
      keto_write_url: draft.keto_write_url,
      keto_opl_url: draft.keto_opl_url,
    };
    try {
      if (draft.id) {
        await api(`/api/console/environments/${draft.id}`, {
          method: "PUT",
          body: JSON.stringify(body),
        });
      } else {
        await api("/api/console/environments", {
          method: "POST",
          body: JSON.stringify(body),
        });
      }
      setDraft(EMPTY_DRAFT);
      await reload();
    } catch (err) {
      setFormError(
        err instanceof ApiError && err.status === 400
          ? "Check the name and URLs. Each address must be an http(s) origin with no path, such as http://kratos:4434. Keto URLs are optional, but all three must be filled together."
          : "Could not save this contour.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function remove(env: Environment) {
    if (!window.confirm(`Delete contour “${env.name}”?`)) return;
    setBusy(true);
    setFormError(null);
    try {
      await api(`/api/console/environments/${env.id}`, { method: "DELETE" });
      if (draft.id === env.id) setDraft(EMPTY_DRAFT);
      if (data?.active_id === env.id) {
        window.location.reload();
        return;
      }
      await reload();
    } catch {
      setFormError("Could not delete this contour.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Contours</DialogTitle>
          <DialogDescription>
            Saved on this admin account. Local always uses the addresses this
            console was started with. A contour needs the Kratos admin API, the
            Hydra admin API, and the Hydra public API.
          </DialogDescription>
        </DialogHeader>

        <ul className="max-h-40 space-y-2 overflow-auto">
          {(data?.environments.length ?? 0) === 0 ? (
            <li className="text-muted-foreground text-sm">
              No extra contours yet.
            </li>
          ) : (
            data?.environments.map((env) => (
              <li
                key={env.id}
                className="flex items-center justify-between gap-2 rounded-md border px-3 py-2"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium">
                      {env.name}
                    </span>
                    {isProdName(env.name) ? (
                      <Badge variant="destructive">prod</Badge>
                    ) : null}
                    {data.active_id === env.id ? (
                      <Badge variant="secondary">active</Badge>
                    ) : null}
                  </div>
                  <p className="text-muted-foreground truncate text-xs">
                    {env.kratos_admin_url}
                  </p>
                </div>
                <div className="flex shrink-0 gap-1">
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => edit(env)}
                  >
                    Edit
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Delete ${env.name}`}
                    onClick={() => void remove(env)}
                    disabled={busy}
                  >
                    <Trash2 />
                  </Button>
                </div>
              </li>
            ))
          )}
        </ul>

        <form
          className="grid gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <div className="flex items-center justify-between">
            <p className="text-sm font-medium">
              {draft.id ? "Edit contour" : "New contour"}
            </p>
            {draft.id ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  setDraft(EMPTY_DRAFT);
                  setFormError(null);
                }}
              >
                <Plus />
                New
              </Button>
            ) : null}
          </div>
          <Field
            label="Name"
            value={draft.name}
            placeholder="Test"
            onChange={(name) => setDraft({ ...draft, name })}
          />
          <Field
            label="Kratos admin URL"
            value={draft.kratos_admin_url}
            placeholder="http://kratos:4434"
            onChange={(kratos_admin_url) =>
              setDraft({ ...draft, kratos_admin_url })
            }
          />
          <Field
            label="Hydra admin URL"
            value={draft.hydra_admin_url}
            placeholder="http://hydra:4445"
            onChange={(hydra_admin_url) =>
              setDraft({ ...draft, hydra_admin_url })
            }
          />
          <Field
            label="Hydra public URL"
            value={draft.hydra_public_url}
            placeholder="http://hydra:4444"
            onChange={(hydra_public_url) =>
              setDraft({ ...draft, hydra_public_url })
            }
          />
          <p className="text-muted-foreground text-xs">
            Keto is optional. Leave all three empty to keep permissions on
            Local, or set read (:4466), write (:4467), and OPL (:4469) together.
          </p>
          <Field
            label="Keto read URL"
            value={draft.keto_read_url}
            placeholder="http://keto:4466"
            onChange={(keto_read_url) => setDraft({ ...draft, keto_read_url })}
          />
          <Field
            label="Keto write URL"
            value={draft.keto_write_url}
            placeholder="http://keto:4467"
            onChange={(keto_write_url) =>
              setDraft({ ...draft, keto_write_url })
            }
          />
          <Field
            label="Keto OPL URL"
            value={draft.keto_opl_url}
            placeholder="http://keto:4469"
            onChange={(keto_opl_url) => setDraft({ ...draft, keto_opl_url })}
          />
          {formError ? (
            <p className="text-destructive text-sm" role="alert">
              {formError}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="submit" disabled={busy}>
              {busy ? "Saving…" : draft.id ? "Save changes" : "Add contour"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function Field({
  label,
  value,
  placeholder,
  onChange,
}: {
  label: string;
  value: string;
  placeholder: string;
  onChange: (value: string) => void;
}) {
  const id = label.toLowerCase().replace(/[^a-z]+/g, "-");
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        value={value}
        placeholder={placeholder}
        autoComplete="off"
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}
