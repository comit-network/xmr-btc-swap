import { Typography } from "@material-ui/core";
import MoneroIcon from "../../icons/MoneroIcon";
import DepositAddressInfoBox from "../../modal/swap/DepositAddressInfoBox";

const XMR_DONATE_ADDRESS =
  "87jS4C7ngk9EHdqFFuxGFgg8AyH63dRUoULshWDybFJaP75UA89qsutG5B1L1QTc4w228nsqsv8EjhL7bz8fB3611Mh98mg";

export default function DonateInfoBox() {
  return (
    <DepositAddressInfoBox
      title="Donate"
      address={XMR_DONATE_ADDRESS}
      icon={<MoneroIcon />}
      additionalContent={
        <Typography variant="subtitle2">
          We rely on generous donors like you to keep development moving
          forward. To bring Atomic Swaps to life, we need resources. If you have
          the possibility, please consider making a donation to the project. All
          funds will be used to support contributors and critical
          infrastructure.
        </Typography>
      }
    />
  );
}
