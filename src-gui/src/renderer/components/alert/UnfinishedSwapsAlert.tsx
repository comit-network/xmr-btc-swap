import { Button } from "@mui/material";
import Alert from "@mui/material/Alert";
import { useNavigate } from "react-router-dom";
import { useResumeableSwapsCountExcludingPunished } from "store/hooks";

export default function UnfinishedSwapsAlert() {
  const resumableSwapsCount = useResumeableSwapsCountExcludingPunished();
  const navigate = useNavigate();

  if (resumableSwapsCount > 0) {
    return (
      <Alert
        severity="warning"
        variant="filled"
        action={
          <Button
            variant="outlined"
            size="small"
            onClick={() => navigate("/history")}
          >
            VIEW
          </Button>
        }
      >
        You have{" "}
        {resumableSwapsCount > 1
          ? `${resumableSwapsCount} pending swaps`
          : "one pending swap"}
      </Alert>
    );
  }
  return null;
}
