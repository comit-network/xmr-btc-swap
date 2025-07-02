import { Box, Typography, styled } from "@mui/material";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import { useSettings } from "store/hooks";
import { Search } from "@mui/icons-material";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { listSellersAtRendezvousPoint } from "renderer/rpc";
import { useAppDispatch } from "store/hooks";
import { discoveredMakersByRendezvous } from "store/features/makersSlice";
import { useSnackbar } from "notistack";

const StyledPromiseButton = styled(PromiseInvokeButton)(({ theme }) => ({
  marginTop: theme.spacing(2),
}));

export default function DiscoveryBox() {
  const rendezvousPoints = useSettings((s) => s.rendezvousPoints);
  const dispatch = useAppDispatch();
  const { enqueueSnackbar } = useSnackbar();

  const handleDiscovery = async () => {
    const { sellers } = await listSellersAtRendezvousPoint(rendezvousPoints);
    dispatch(discoveredMakersByRendezvous(sellers));
  };

  return (
    <InfoBox
      title={
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          Discover Makers
        </Box>
      }
      mainContent={
        <Typography variant="subtitle2">
          By connecting to rendezvous points run by volunteers, you can discover
          makers and then connect and swap with them in a decentralized manner.
          You have {rendezvousPoints.length} stored rendezvous{" "}
          {rendezvousPoints.length === 1 ? "point" : "points"} which we will
          connect to. We will also attempt to connect to peers which you have
          previously connected to.
        </Typography>
      }
      additionalContent={
        <StyledPromiseButton
          variant="contained"
          color="primary"
          onInvoke={handleDiscovery}
          disabled={rendezvousPoints.length === 0}
          startIcon={<Search />}
          displayErrorSnackbar
        >
          Discover Makers
        </StyledPromiseButton>
      }
      icon={null}
      loading={false}
    />
  );
}
