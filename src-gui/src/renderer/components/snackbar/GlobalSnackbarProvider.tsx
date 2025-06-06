import { IconButton, styled } from "@mui/material";
import { Close } from "@mui/icons-material";
import {
  MaterialDesignContent,
  SnackbarKey,
  SnackbarProvider,
  useSnackbar,
} from "notistack";
import { ReactNode } from "react";

const StyledMaterialDesignContent = styled(MaterialDesignContent)(() => ({
  "&.notistack-MuiContent": {
    maxWidth: "50vw",
  },
}));

function CloseSnackbarButton({ snackbarId }: { snackbarId: SnackbarKey }) {
  const { closeSnackbar } = useSnackbar();

  return (
    <IconButton onClick={() => closeSnackbar(snackbarId)} size="large">
      <Close />
    </IconButton>
  );
}

export default function GlobalSnackbarManager({
  children,
}: {
  children: ReactNode;
}) {
  return (
    <SnackbarProvider
      action={(snackbarId) => <CloseSnackbarButton snackbarId={snackbarId} />}
      Components={{
        success: StyledMaterialDesignContent,
        error: StyledMaterialDesignContent,
        default: StyledMaterialDesignContent,
        info: StyledMaterialDesignContent,
        warning: StyledMaterialDesignContent,
      }}
    >
      {children}
    </SnackbarProvider>
  );
}
