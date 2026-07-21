// Shared Tailwind class constants

// Layout
export const card =
  "rounded-lg border border-border bg-card p-6 shadow-md hover:shadow-lg transition-shadow";
export const cardInner =
  "border border-border rounded-lg bg-background p-4 shadow-sm";

// Form inputs
export const selectInput =
  "w-full px-3 py-2 border border-border rounded-md bg-input text-foreground shadow-sm focus:outline-none focus:ring-2 focus:ring-brand-cyan/50 focus:border-brand-cyan transition-colors";  

export const textInput = `${selectInput} placeholder:text-muted-foreground`;
export const textArea =
  "w-full px-3 py-2 rounded-md border border-border bg-input text-foreground placeholder:text-muted-foreground shadow-sm focus:outline-none focus:ring-2 focus:ring-brand-cyan/50 focus:border-brand-cyan transition-colors resize-vertical min-h-[80px]";
export const textareaInput =
  "w-full min-h-[100px] px-3 py-2 rounded-md border border-border bg-input text-foreground placeholder:text-muted-foreground shadow-sm focus:outline-none focus:ring-2 focus:ring-brand-cyan/50 focus:border-brand-cyan transition-colors resize-y";
export const confirmInput =
  "w-full px-3 py-2 border border-border rounded-md bg-input text-foreground placeholder:text-muted-foreground shadow-sm focus:outline-none focus:ring-2 focus:ring-destructive focus:border-transparent mb-4";
export const radioInput =
  "w-4 h-4 text-brand-cyan border-border focus:ring-brand-cyan";
export const fieldLabel = "block text-sm font-medium text-foreground mb-1";

// Buttons
export const btnPrimary =
  "px-6 py-2.5 bg-primary text-primary-foreground rounded-md hover:bg-primary/85 shadow-md hover:shadow-lg hover:shadow-primary/20 transition-all font-semibold disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2";
export const btnDanger =
  "px-4 py-2 border border-destructive/30 text-destructive rounded-md hover:bg-destructive/10 transition-colors font-medium flex items-center gap-2 text-sm";
export const btnSecondary =
  "px-4 py-2 border border-border text-foreground rounded-md hover:bg-muted/50 transition-colors font-medium flex items-center gap-2 text-sm";
export const btnCopy =
  "flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors px-2 py-1 rounded border border-border";
export const btnDelete =
  "px-2.5 py-1 text-xs font-medium rounded-md border border-destructive/30 text-destructive hover:bg-destructive/10 disabled:opacity-50 transition-colors";
export const btnDeleteSmall =
  "px-2 py-0.5 text-xs font-medium rounded-md border border-destructive/30 text-destructive hover:bg-destructive/10 transition-colors";
export const btnIconDelete =
  "p-1.5 rounded-md text-destructive hover:bg-destructive/10 disabled:opacity-50 transition-colors [&_svg]:w-4 [&_svg]:h-4";
export const btnCancel =
  "px-4 py-2 text-sm rounded-md border border-border text-foreground hover:bg-muted transition-colors";
export const btnConfirm =
  "px-4 py-2 text-sm font-medium rounded-md bg-destructive text-white hover:bg-destructive/90 disabled:opacity-50 transition-colors";

// Alerts
export const alertSuccess =
  "mb-4 p-3 rounded-md bg-success/20 border-2 border-success/70 text-success dark:bg-success/10 dark:border dark:border-success/30 dark:text-success text-sm font-medium";
export const alertError =
  "mb-4 p-3 rounded-md bg-destructive/10 text-destructive text-sm";
export const alertValidation = "text-xs text-destructive mt-1";

// Modals
export const modalOverlay =
  "fixed inset-0 z-50 flex items-center justify-center bg-black/50";
export const modalPanel =
  "rounded-lg border border-border bg-card p-6 max-w-md w-full mx-4 shadow-xl";

// Data display
export const codeBlock =
  "px-3 py-2 rounded-md border border-border bg-muted text-sm text-foreground font-mono break-all shadow-inner";
export const badge = "px-1.5 py-0.5 bg-primary/10 text-primary rounded text-xs";
export const loader = "flex items-center gap-2 text-muted-foreground";
export const statusDot = "inline-block w-2 h-2 rounded-full shrink-0";

// Table cells
export const cellBase = "py-1.5 px-2";
export const thCell = `text-left ${cellBase} font-medium text-muted-foreground`;

// Slider
export const sliderBounds =
  "flex justify-between text-xs text-muted-foreground mt-1";

// Dropzone file rows
const iconBox =
  "h-10 w-10 rounded border flex items-center justify-center shrink-0";
export const successIconBox = `${iconBox} border-success/70 bg-success/20 dark:border-success/30 dark:bg-success/10`;
export const fileIconBox = `${iconBox} border-border bg-muted`;
export const fileRow =
  "flex items-center gap-x-4 border-b border-border py-2 first:mt-4 last:mb-4";

// Wizard
export const wizardStepDot =
  "w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold transition-colors";
export const wizardStepDotActive =
  "bg-primary text-primary-foreground shadow-md shadow-primary/30";
export const wizardStepDotCompleted =
  "bg-success text-success-foreground";
export const wizardStepDotPending =
  "bg-muted text-muted-foreground border border-border";
export const wizardStepLabel =
  "text-xs mt-1 font-medium transition-colors";
export const wizardConnector =
  "flex-1 h-0.5 mx-2 rounded transition-colors";
export const wizardNav =
  "flex items-center justify-between mt-6 pt-4 border-t border-border";
export const wizardCard =
  "rounded-lg border-2 p-5 cursor-pointer transition-all";
export const wizardCardSelected =
  "border-primary bg-primary/5 shadow-md shadow-primary/10";
export const wizardCardUnselected =
  "border-border hover:border-primary/40 hover:bg-muted/30";
