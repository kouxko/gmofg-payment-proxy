/*
 *  = COPYRIGHT
 *           PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *    This software is supplied under the terms of a license agreement or nondisclosure
 *    agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *    disclosed except in accordance with the terms in that agreement.
 *    Copyright (C) 2019-? Computer Technology(Shenzhen) CO., LTD All rights reserved.
 */
package jp.gmofg.payment.pay.trans;

import static jp.gmofg.payment.util.JVMAUtilKt.isJVMA;

import android.app.Activity;
import android.os.SystemClock;
import android.text.TextUtils;
import android.util.Log;
import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.annotation.WorkerThread;
import androidx.fragment.app.FragmentActivity;
import com.pax.dal.entity.EPiccType;
import com.pax.dal.exceptions.AGeneralException;
import java.util.concurrent.atomic.AtomicBoolean;
import jp.gmofg.common.util.PLog;
import jp.gmofg.common.util.StringUtil;
import jp.gmofg.common.util.TrackUtil;
import jp.gmofg.core.ActivityStack;
import jp.gmofg.core.AppThreadExecutor;
import jp.gmofg.core.NeptuneLiteProvider;
import jp.gmofg.customview.dialog.PromptDialog;
import jp.gmofg.customview.util.ScreenID;
import jp.gmofg.customview.util.Utils;
import jp.gmofg.emv.EmvImpl;
import jp.gmofg.emv.IClss;
import jp.gmofg.emv.IEmv;
import jp.gmofg.emv.clss.ClssImpl;
import jp.gmofg.emv.entity.CTransResult;
import jp.gmofg.emv.enums.ETransResult;
import jp.gmofg.emv.exception.EEmvExceptions;
import jp.gmofg.emv.exception.EmvException;
import jp.gmofg.msg.adapter.PanInputMode;
import jp.gmofg.msg.receipt.JsonReceipt;
import jp.gmofg.msg.trans.payment.PaymentMethod;
import jp.gmofg.payment.BuildConfig;
import jp.gmofg.payment.R;
import jp.gmofg.payment.core.ActionResult;
import jp.gmofg.payment.core.EventBusProvider;
import jp.gmofg.payment.core.PosLinkClientProvider;
import jp.gmofg.payment.data.entity.TransData;
import jp.gmofg.payment.data.entity.TransReceipt;
import jp.gmofg.payment.data.entity.UnionPayTransData;
import jp.gmofg.payment.data.entity.inteface.ITransData;
import jp.gmofg.payment.data.local.Repository;
import jp.gmofg.payment.dialog.DialogUtils;
import jp.gmofg.payment.domain.usecase.LauncherAppSettingsUseCase;
import jp.gmofg.payment.eventbus.DetectCardEvent;
import jp.gmofg.payment.https.client.HttpsClient;
import jp.gmofg.payment.pay.trans.action.ActionEmvProcess;
import jp.gmofg.payment.pay.trans.action.ActionRemoveCard;
import jp.gmofg.payment.pay.trans.action.ActionSearchCard;
import jp.gmofg.payment.pay.trans.action.activity.searchcard.SearchCardActivity;
import jp.gmofg.payment.pay.trans.component.Component;
import jp.gmofg.payment.pay.trans.credit.CreditPreAuthCompOfflineTrans;
import jp.gmofg.payment.pay.trans.model.TransType;
import jp.gmofg.payment.pay.trans.receipt.JsonReceiptRequestConverter;
import jp.gmofg.payment.pay.trans.task.BaseTask;
import jp.gmofg.payment.pay.trans.task.PrintErrorTask;
import jp.gmofg.payment.pay.trans.task.PrintTask;
import jp.gmofg.payment.pay.trans.transmit.activity.OnlineActivity;
import jp.gmofg.payment.pay.utils.AndroidUtil;
import jp.gmofg.payment.pay.utils.CurrencyConverter;
import jp.gmofg.payment.pay.utils.PaymentMethodUtil;
import jp.gmofg.payment.pay.utils.ReaderTypeHelper;
import jp.gmofg.payment.pay.utils.TransUtils;
import jp.gmofg.payment.poslink.PosLinkEndHandler;
import jp.gmofg.payment.poslink.PosLinkMessageGenerator;
import jp.gmofg.payment.poslink.PosLinkTransEndHandler;
import jp.gmofg.payment.poslink.PosLinkUtil;
import jp.gmofg.payment.poslink.handler.PosLinkLastTransHandler;
import jp.gmofg.payment.poslink.handler.PosLinkMsgRememberHandler;
import jp.gmofg.payment.receiver.SendDataParam;
import jp.gmofg.payment.settings.AppPreference;
import jp.gmofg.payment.settings.LauncherSettings;
import jp.gmofg.payment.util.AffiliationDataUtil;
import jp.gmofg.poslink.message.PosLinkMessage;

/**
 * The type Trans.
 */
public abstract class Trans extends BaseTask {

    public static final int FLAG_DONT_CLEAR_REMEMBER_CARD_INFO = 0x1234ABCD;

    /**
     * session container;
     */
    protected static final SessionContainer INSTANCE = new SessionContainer();
    private static final String TAG = "Trans";
    /**
     * MERCHANT INFO
     */
    private static final int MERCHANT_INFO_NONE = 0;
    public static int MODE3_TIME_OUT = 2;
    private static boolean isFirstUseCamera = true;
    private static boolean isContinueBill = false;
    /**
     * amount
     */
    protected final int mAmount;
    /**
     * tax other
     */
    protected final int mTaxOther;
    private final TransType transType;
    /**
     * The Emv.
     */
    protected IEmv mEmv = new EmvImpl();
    /**
     * The Clss.
     */
    protected IClss mClss = new ClssImpl();
    /**
     * transData;
     */
    protected ITransData mTransData;
    /**
     * sign data
     */
    protected byte[] mSignData;
    /**
     * search card mode
     */
    protected byte mSearchCardMode;
    /**
     * The Need remove card.
     */
    protected boolean mNeedRemoveCard = false;
    private volatile Runnable checkOneTimeUnsentTask = () -> {
        //交易结束后进行离线检查上送
        AtomicBoolean isCheckFinished = new AtomicBoolean(false);
        TransUtils.checkUnSend(null, true, true, ret -> isCheckFinished.set(true));

        while (!isCheckFinished.get()) {
            SystemClock.sleep(100);
        }

        PLog.i("Trans", "checkUnsentTask finished");
    };
    /**
     * back to main
     */
    private boolean mBackToMain = false;

    protected Trans(final TransType transType, boolean isAttendant) {
        super(null);
        isContinueBill = false;
        this.mSearchCardMode = transType.getReadMode();
        this.mAmount = 0;
        this.mTaxOther = 0;
        this.transType = transType;
        INSTANCE.setNeedFallback(false);
        INSTANCE.setTransRunning(false);
        INSTANCE.setPosLinkMode(false);
        INSTANCE.setAttendantMode(isAttendant);
        SendDataParam.enableSendData(false);
        initTrans(transType);
        isContinueBill = false;
    }

    protected Trans(final TransType transType, final int amount, final int taxOther,
                    boolean isAttendant) {
        this(transType, amount, taxOther, isAttendant, null);
    }

    protected Trans(final TransType transType, final int amount, final int taxOther,
                    boolean isAttendant, final TransEndListener transEndListener) {
        super(transEndListener);
        this.mSearchCardMode = transType.getReadMode();
        this.mAmount = amount;
        this.mTaxOther = taxOther;
        this.transType = transType;
        INSTANCE.setNeedFallback(false);
        INSTANCE.setTransRunning(false);
        INSTANCE.setPosLinkMode(true);
        INSTANCE.setAttendantMode(isAttendant);
        SendDataParam.enableSendData(false);
        initTrans(transType);
        isContinueBill = false;
    }

    public static boolean takeFirstTimeUseCamera() {
        boolean isFirst = isFirstUseCamera;
        isFirstUseCamera = false;
        return isFirst;
    }

    /**
     * get transaction running status
     *
     * @return boolean boolean
     */
    public static boolean isTransRunning() {
        return INSTANCE.isTransRunning();
    }

    /**
     * set transaction running status
     *
     * @param isTransRunning the is trans running
     */
    public static void setTransRunning(boolean isTransRunning) {
        INSTANCE.setTransRunning(isTransRunning);
    }

    /**
     * Gets current session.
     *
     * @return the current session
     */
    public static SessionContainer getCurrentSession() {
        return INSTANCE;
    }

    public static void clearSession() {
        INSTANCE.clear();
    }

    public static boolean isTankingOverLimit() {
        Repository repository = Repository.getInstance();
        int tankingDataLimit = AppPreference.getMaxTankingDataLimit();
        long tankData = repository.countAllTankData();
        PLog.i(TAG, "Max tanking data limit: " + tankingDataLimit + ", tank data count: " + tankData);
        return tankData >= tankingDataLimit;
    }

    /**
     * Gets current trans data.
     *
     * @return the current trans data
     */
    @Nullable
    public static ITransData getCurrentTransData() {
        return INSTANCE.getTransData();
    }

    public static boolean hasTransData() {
        return INSTANCE.getTransData() != null;
    }

    /**
     * Is need merchant info boolean.
     *
     * @return the boolean
     */
    public static boolean isNeedMerchantInfo() {
        return AppPreference.getMerchantInfoIndex() != MERCHANT_INFO_NONE;
    }

    /**
     * Require current trans data trans data.
     *
     * @return the trans data
     */
    @NonNull
    public static ITransData requireCurrentTransData() {
        ITransData transData = INSTANCE.getTransData();
        if (transData == null) {
            throw new IllegalStateException("trans data is null");
        }
        return transData;
    }

    /**
     * Gets current trans type.
     *
     * @return the current trans type
     */
    @Nullable
    public static TransType getCurrentTransType() {
        ITransData transData = getCurrentTransData();
        if (transData == null) {
            return null;
        }
        return transData.getTransType();
    }

    @Nullable
    public static PanInputMode getCurrentPanInputMode() {
        ITransData transData = getCurrentTransData();
        if (transData == null) {
            return null;
        }
        return transData.getPanInputMode();
    }

    /**
     * Require current trans type transaction.
     *
     * @return the transaction
     */
    @NonNull
    public static TransType requireCurrentTransType() {
        TransType transType = requireCurrentTransData().getTransType();
        if (transType == null) {
            throw new IllegalStateException("trans type is null");
        }
        return transType;
    }

    /**
     * Gets current trans name.
     *
     * @return the current trans name
     */
    @Nullable
    public static String getCurrentTransName() {
        TransType transType = getCurrentTransType();
        if (transType == null) {
            return null;
        }
        return transType.getTransName();
    }

    /**
     * Require current trans name string.
     *
     * @return the string
     */
    @NonNull
    public static String requireCurrentTransName() {
        String transName = requireCurrentTransType().getTransName();
        if (transName == null) {
            throw new IllegalStateException("trans name is null");
        }
        return transName;
    }

    /**
     * Require current format total amount string.
     *
     * @return the string
     */
    @NonNull
    public static String requireCurrentFormatTotalAmount() {
        return CurrencyConverter.convert(
                (long) requireCurrentTransData().getAmount() + requireCurrentTransData().getTax());
    }

    /**
     * Require current format total amount string.
     *
     * @param isNegative the is negative
     * @return the string
     */
    @NonNull
    public static String requireCurrentFormatTotalAmount(boolean isNegative) {
        return CurrencyConverter.convert(isNegative ? -(requireCurrentTransData().getAmount()
                + requireCurrentTransData().getTax())
                : requireCurrentTransData().getAmount() + requireCurrentTransData().getTax());
    }

    /**
     * Is need fall back boolean.
     *
     * @return the boolean
     */
    public static boolean isNeedFallBack() {
        return INSTANCE.isNeedFallback();
    }

    public static void setNeedFallBack(boolean fallback) {
        INSTANCE.setNeedFallback(fallback);
    }

    /**
     * send json receipt
     */
    public static void sendJsonReceipt() {
        //电子签名开关关闭不上送
        if (!AppPreference.useElectronicSignature()) return;

        AppThreadExecutor.runInExecutor(() -> {
            for (TransReceipt transReceipt : Repository.getInstance().findUnSendTransReceipt()) {
                try {
                    JsonReceipt.Response response = HttpsClient.getFinancialGateService()
                            .sendJsonReceipt(AppPreference.getDllHostAddress(),
                                    JsonReceiptRequestConverter.convert(transReceipt))
                            .execute()
                            .body();
                    if (response != null && TextUtils.equals(response.getErrorCode(), "   ")) {
                        PLog.d("Tran receipt", transReceipt.getTransNo() + "had send!");
                        transReceipt.setSendFlag(true);
                        Repository.getInstance().insertOrReplaceTransReceipt(transReceipt);
                    }
                } catch (Exception e) {
                    PLog.d(e);
                }
            }
        });
    }

    /**
     * is continue bill
     *
     * @return continue bill
     */
    public static boolean isContinueBill() {
        return isContinueBill;
    }

    /**
     * set continue bill
     *
     * @param continueBill continue bill
     */
    public void setContinueBill(boolean continueBill) {
        isContinueBill = continueBill;
    }

    /**
     * is attendant
     *
     * @return attendant
     */
    public static boolean isAttendant() {
//        if (Utils.isUPT()) {       //Jerry mask 20241210：需要修改回POSLINK原有功能。
//            return false;
//        }
        return INSTANCE.isAttendantMode();
    }

    public static boolean isPosLinkMode() {
        return INSTANCE.isPosLinkMode();
    }

    public static boolean isCurrentCreditVoidTrans() {
        ITransData transData = requireCurrentTransData();
        if (transData.isUnionPay()) return false;
        return transData.getTransType() == TransType.VOID;
    }

    public static boolean isCurrentCreditTrans() {
        final ITransData transData = getCurrentTransData();
        if (null == transData) {
            return true;
        }
        return !transData.isUnionPay();
    }

    public static boolean isCurrentCreditExtTrans() {
        SessionContainer currentSession = getCurrentSession();
        PosLinkMessage.Header posLinkHeader = currentSession.getPosLinkHeader();
        if (posLinkHeader == null) return false;
        return TextUtils.equals("11", posLinkHeader.getBusinessCategory());
    }

    public static boolean isCurrentCreditSaleTrans() {
        ITransData transData = requireCurrentTransData();
        if (transData.isUnionPay()) return false;
        return transData.getTransType() == TransType.SALE;
    }

    public static boolean isCurrentPaymentMethodNone() {
        ITransData transData = requireCurrentTransData();
        PaymentMethod paymentMethod = transData.getPaymentMethod();
        return paymentMethod == null || paymentMethod.isNone();
    }

    public static boolean isCurrentPaymentMethodSinglePay() {
        ITransData transData = requireCurrentTransData();
        PaymentMethod paymentMethod = transData.getPaymentMethod();
        return paymentMethod != null && paymentMethod.isSinglePay();
    }

    public static boolean isPosLinkRequiredSearchCardType() {
        return PosLinkUtil.tryGetPosLinkRequireSearchCardType() != -1;
    }

    public static boolean isVoidTransPaymentMethodDeclaredByPosLink() {
        return PaymentMethodUtil.isVoidTransPaymentMethodDeclaredByPosLink();
    }

    /**
     * 返回maxstore下发的ms control fallback 控制是否允许fallback
     */
    public static boolean isAllowCreditFallback() {
        return AppPreference.isCreditMSFallback();
    }

    public static boolean isAllowUnionPayFallback() {
        return AppPreference.isAllowUnionPayFallback();
    }

    public static boolean isCurrentUnionPayTrans() {
        return requireCurrentTransData().isUnionPay();
    }

    public static void resetDefaultTransAllowFallback() {
        ITransData transData = getCurrentTransData();
        if (transData == null) {
            return;
        }

        if (transData.isUnionPay()) {
            transData.setSupportFallback(isAllowUnionPayFallback());
        }
        transData.setSupportFallback(isAllowCreditFallback());
    }

    public static boolean isCurrentMITSUICheckCard() {
        return requireCurrentTransData().getTransType() == TransType.CARD_MITSUI_CHECK;
    }

    public static boolean isAllowFallback() {
        if (isJVMA()) return false;
        //poslink指定了支付方式 不允许fallback 要按照poslink的指定走
        if (PosLinkUtil.tryGetPosLinkRequireSearchCardType() != -1) return false;
        //如果是三菱信用卡检查交易 不允许fallback
        if (isCurrentMITSUICheckCard()) return false;

        ITransData currentTransData = getCurrentTransData();
        if (currentTransData != null) {
            if (currentTransData.getPanInputMode() == PanInputMode.CONTACT_LESS_IC) {
                return true;
            }
            Boolean supportFallback = currentTransData.isSupportFallback();
            if (supportFallback != null) {
                return supportFallback;
            }
        }

        if (isCurrentUnionPayTrans()) {
            return isAllowUnionPayFallback();
        } else {
            return isAllowCreditFallback();
        }
    }

    @WorkerThread
    public static void trySaveLastDefaultSignSuccessTransPosLinkMsg() {
        ITransData transData = Trans.getCurrentTransData();
        if (transData instanceof TransData) {
            ((TransData) transData).setSignData(new byte[0]);
        }
        if (transData instanceof UnionPayTransData) {
            ((UnionPayTransData) transData).setSignData(new byte[0]);
        }
        final TransReceipt receipt = TransReceipt.from(transData);
        receipt.setSignData(new byte[]{});
        Repository.getInstance().insertOrReplaceTransReceipt(receipt);
        trySaveLastSuccessTransPosLinkMsg();
    }

    /**
     * 如果交易时成功的，调用该方法保留需要回复的poslink报文到本地
     */
    @WorkerThread
    public static void trySaveLastSuccessTransPosLinkMsg() {
        ITransData currentTransData = Trans.getCurrentTransData();
        if (currentTransData == null) return;
        SessionContainer currentSession = Trans.getCurrentSession();
        if (currentSession == null) return;
        PosLinkMessage.Header header = currentSession.getPosLinkHeader();
        if (header == null) return;
        String business = header.getBusinessCategory();
        String transaction = header.getTransactionType();
        if (!PosLinkLastTransHandler.isNeedSaveFunction(transaction)) return;
        PLog.i("Trans", "Save last success poslink message");
        TransReceipt receipt = TransReceipt.from(currentTransData);
        PosLinkTransEndHandler posLinkTransEndHandler = new PosLinkTransEndHandler();
        String message = posLinkTransEndHandler.handle(PosLinkMessage.Header.copy(header), new ActionResult(TransResult.SUCC, receipt));
        PosLinkLastTransHandler.saveLastPosLinkMessage(business, transaction, message);
    }

    /**
     * 如果交易时成功的，调用该方法保留需要回复的poslink报文到本地
     */
    @WorkerThread
    public static void trySaveLastSuccessOrErrorTransPosLinkMsg() {
        ITransData currentTransData = Trans.getCurrentTransData();
        if (currentTransData == null) return;
        SessionContainer currentSession = Trans.getCurrentSession();
        if (currentSession == null) return;
        PosLinkMessage.Header header = currentSession.getPosLinkHeader();
        if (header == null) return;
        String business = header.getBusinessCategory();
        String transaction = header.getTransactionType();
        if (!PosLinkLastTransHandler.isNeedSaveFunction(transaction)) return;
        PLog.i("Trans", "trySaveLastSuccessOrErrorTransPosLinkMsg");
        TransReceipt receipt = TransReceipt.from(currentTransData);
        String message;
        PosLinkTransEndHandler posLinkTransEndHandler = new PosLinkTransEndHandler();
        if (currentTransData.isSuccessful()) {
            message = posLinkTransEndHandler.handle(PosLinkMessage.Header.copy(header), new ActionResult(TransResult.SUCC, receipt));
        } else {
            PosLinkMessage.Header retHeader = PosLinkMessage.Header.copy(header);
            retHeader.setResultCode(currentTransData.getErrCode());

            PosLinkMessageGenerator<TransReceipt> generator;
            if (currentTransData.isUnionPay()) {
                generator = new PosLinkMessageGenerator.UnionPayMessageGenerator();
            } else {
                generator = new PosLinkMessageGenerator.CreditMessageGenerator();
            }
            message = generator.generate(retHeader, receipt);
        }
        PosLinkLastTransHandler.saveLastPosLinkMessage(business, transaction, message);
    }

    @WorkerThread
    public static void trySaveLastTransPosLinkMsg(String errorCode) {
        ITransData currentTransData = Trans.getCurrentTransData();
        if (currentTransData == null) return;
        SessionContainer currentSession = Trans.getCurrentSession();
        if (currentSession == null) return;
        PosLinkMessage.Header header = currentSession.getPosLinkHeader();
        if (header == null) return;
        String business = header.getBusinessCategory();
        String transaction = header.getTransactionType();
        if (!PosLinkLastTransHandler.isNeedSaveFunction(transaction)) return;
        TransReceipt receipt = TransReceipt.from(currentTransData);
        PosLinkMessage.Header retHeader = PosLinkMessage.Header.copy(header);
        retHeader.setResultCode(errorCode);
        PosLinkMessageGenerator<TransReceipt> generator;
        if (currentTransData.isUnionPay()) {
            generator = new PosLinkMessageGenerator.UnionPayMessageGenerator();
        } else {
            generator = new PosLinkMessageGenerator.CreditMessageGenerator();
        }
        String message = generator.generate(retHeader, receipt);
        PosLinkLastTransHandler.saveLastPosLinkMessage(business, transaction, message);
    }

    /**
     * 如果交易时成功的，调用该方法保留需要回复的poslink报文到本地
     */
    @WorkerThread
    public static void trySaveLastFailedTransPosLinkMsg(int transResult) {
        ITransData currentTransData = Trans.getCurrentTransData();
        if (currentTransData == null) return;
        SessionContainer currentSession = Trans.getCurrentSession();
        if (currentSession == null) return;
        PosLinkMessage.Header header = currentSession.getPosLinkHeader();
        if (header == null) return;
        String business = header.getBusinessCategory();
        String transaction = header.getTransactionType();
        if (!PosLinkLastTransHandler.isNeedSaveFunction(transaction)) return;
        PLog.i("Trans", "Save last success poslink message");
        TransReceipt receipt = TransReceipt.from(currentTransData);
        PosLinkTransEndHandler posLinkTransEndHandler = new PosLinkTransEndHandler();
        String message = posLinkTransEndHandler.handle(PosLinkMessage.Header.copy(header), new ActionResult(transResult, receipt));
        PosLinkLastTransHandler.saveLastPosLinkMessage(business, transaction, message);
    }

    public static void clearCardInfoWhenResearch() {
        ITransData currentTransData = getCurrentTransData();
        if (currentTransData != null) {
            currentTransData.clearCardInfoWhenResearch();
        }
    }

    private static void tryUpdateKidFromWhiteList() {
        final ITransData currentTransData = Trans.getCurrentTransData();
        if (currentTransData == null) {
            return;
        }
        final String kid = AffiliationDataUtil.findKid(currentTransData);
        if (kid == null) {
            return;
        }
        currentTransData.setKid(kid);
    }

    /**
     * init trans
     *
     * @param transType transaction type
     */
    protected abstract void initTrans(TransType transType);

    /**
     * get menu activity class
     *
     * @return menu activity class
     */
    protected abstract Class<? extends FragmentActivity> getMenuActivityClass();

    protected void resetEntryPoint() {
        if (ReaderTypeHelper.isSupportPicc(mSearchCardMode) && ActivityStack.hasActivity(
                SearchCardActivity.class)) {
            try {
                mClss.preTransaction(Component.toClssInputParam(mTransData, AppPreference.getTTQ()));
            } catch (EmvException e) {
                EventBusProvider.getEventBus()
                        .post(DetectCardEvent.disableReader(ReaderTypeHelper.PICC));
            }
        }
    }

    /**
     * Sets back to main.
     */
    protected void setBackToMain() {
        mBackToMain = true;
    }

    /**
     * transaction result prompt
     *
     * @param result the result
     */
    @Override
    protected void transEnd(final ActionResult result) {
        transEnd(result, true);
    }

    protected void transEnd(final ActionResult result, Runnable endTask) {
        transEnd(result, endTask, true);
    }

    protected void transEnd(final ActionResult result, boolean showCancelDialog) {
        //poslink 下取消需要显示dialog
        if (!showCancelDialog && isPosLinkMode()) showCancelDialog = true;
        transEnd(result, null, showCancelDialog);
    }

    /**
     * Trans end.
     *
     * @param result           the result
     * @param showCancelDialog the show cancel dialog
     */
    protected void transEnd(final ActionResult result, Runnable endTask, boolean showCancelDialog) {
        boolean isPosLinkMode = isPosLinkMode();
        boolean isMITSUICheckCard = isCurrentMITSUICheckCard();
        boolean isCreditPreAuthCompOfflineTrans = this instanceof CreditPreAuthCompOfflineTrans;
        PLog.i(TAG, "end transaction: [ " + getCurrentTransName() + " ] " + result.getRet());
        if (BuildConfig.DEBUG) {
            Log.d(TAG, "transEnd called", new Throwable());
        }
        AppThreadExecutor.runInExecutor(() -> {
            int lastTransRet = result.getRet();

            // 三井卡寻卡时会保存卡片信息，但是如果发起了任何交易那么就需要删除保存的信息
            ITransData currentTransData = mTransData;
            if (currentTransData != null) {
                //因为三井卡检查会保存卡片信息，所以不删除
                if (currentTransData.getTransType() != TransType.CARD_MITSUI_CHECK) {
                    Object data = result.getData();
                    boolean requestDontClear = false;
                    if (data instanceof Integer) {
                        requestDontClear = ((int) data == FLAG_DONT_CLEAR_REMEMBER_CARD_INFO);
                    }
                    if (!requestDontClear) {
                        PosLinkUtil.clearRememberSearchCardInfo();
                    }
                }

                boolean isNeedRemember = lastTransRet != TransResult.SUCC;
                PLog.i(PosLinkUtil.TAG, "Ooh, trans not succ,maybe need remember payment method, is need remember: " + isNeedRemember);
                if (isNeedRemember) {
                    isNeedRemember = !currentTransData.isUnionPay();
                    PLog.i(PosLinkUtil.TAG, "if union pay, not need remember payment method, is need remember: " + isNeedRemember);
                }

                if (isNeedRemember) {
                    isNeedRemember = !currentTransData.isRestoredPaymentMethod();
                    PLog.i(PosLinkUtil.TAG, "if restored payment method, not need remember payment method, is need remember: " + isNeedRemember);
                }

                if (isNeedRemember) {
                    String errCode = mTransData.getErrCode();
                    PLog.i(TAG, "trans end result: " + result + ", err code: " + errCode);

                    boolean isGTDError = false;
                    if (errCode != null) {
                        isGTDError = errCode.startsWith("G") || errCode.startsWith("T") || errCode.startsWith("D");
                    }
                    isNeedRemember = isGTDError;
                    PLog.i(PosLinkUtil.TAG, "if GTD error, not need remember payment method, is need remember: " + isNeedRemember);
                }

                //任何交易失败并且不是使用的保存支付方式都尝试记住支付方式
                if (isNeedRemember) {
                    PosLinkUtil.tryRememberPaymentMethodInfo();
                } else {
                    PosLinkUtil.clearRememberPaymentMethodInfo();
                }
            }


            //在emv流程中出现错误后，错误码被放在了data中
            if (TransContext.getCurrentAction() instanceof ActionEmvProcess) {
                Object data = result.getData();
                if (data instanceof Integer) {
                    lastTransRet = (int) data;
                }
            }
            Trans.getCurrentSession().setTransResult(lastTransRet);
            //设置交易结果后生成需要保存的poslink报文
            trySaveLastFailedTransPosLinkMsg(lastTransRet);

            if (!isJVMA()) {
                removeCard();
            }
            displayResult(result, mTransData != null && mTransData.isUnionPay(), () ->
                    AppThreadExecutor.runInExecutor(() -> {
                        int ret = result.getRet();
                        if (endTask != null) {
                            endTask.run();
                        }
                        if (mTransEndListener != null) {
                            ActionResult finalResult = getActionResult(result, ret);
                            if (mTransData != null) {
                                finalResult.setData(Repository.getInstance()
                                        .findTransReceipt(mTransData.isUnionPay(),
                                                mTransData.getTransNo(), mTransData.getTransType()
                                                        == TransType.CONTINUE_BILL));
                            }

                            if (result.getData() instanceof TransReceipt) {
                                finalResult.setData(result.getData());
                            }

                            //没有设置receipt  或者交易没有成功 重新生成receipt 因为从数据里拿的transreceipt是交易成功的
                            if (finalResult.getData() == null || ret != TransResult.SUCC) {
                                finalResult.setData(TransReceipt.from(mTransData));
                            }

                            mTransEndListener.onEnd(finalResult);
                        }
                        if (!isJVMA()) {
                            //关闭画面
                            backToMain();
                            TransContext.clear();
                        }
                        if (isJVMA()) {
                            removeCard();
                        }
                        INSTANCE.clear();
                        clear();


                        //非Demo模式下，如果不是用户取消那么不检查unsent
                        if (LauncherSettings.isTrainingMode()) {
                            return;
                        }
                        if (isCreditPreAuthCompOfflineTrans) {
                            return;
                        }
                        if (isMITSUICheckCard) {
                            return;
                        }
                        if (ret == TransResult.ERR_USER_CANCEL) {
                            return;
                        }
                        if (ret == TransResult.ERR_TIMEOUT) {
                            return;
                        }

                        postCheckUnsentTask();

                    }), showCancelDialog);
            if (Utils.isUPT()) {
                AppThreadExecutor.runOnUiHandler(() -> {
                    Activity top = ActivityStack.top();
                    if (top instanceof OnlineActivity) {
                        ((OnlineActivity) top).showWhiteBoard(true);
                    }
                });
            }
            setTransRunning(false);
            SendDataParam.enableSendData(true);
            try {
                NeptuneLiteProvider.picc(EPiccType.INTERNAL).close();
            } catch (Exception e) {
                //ignore exception
                PLog.d(e);
            }
        });
    }

    private @NonNull ActionResult getActionResult(ActionResult result, int ret) {
        ActionResult finalResult;
        if (mTransEndListener instanceof PosLinkEndHandler) {
            //handle EMV、CLSS result code
            int dataCode = 0;
            try {
                dataCode = result.getData();
            } catch (RuntimeException exception) {
                //do nothing
            }
            if (dataCode == TransResult.ERR_EMV
                    || dataCode == TransResult.ERR_EMV_DENIAL
                    || dataCode == TransResult.ERR_EMV_CLSS
                    || dataCode == TransResult.ERR_EMV_CLSS_DENIAL) {
                finalResult = new ActionResult(dataCode);
            } else {
                finalResult = new ActionResult(ret);
            }
        } else {
            finalResult = new ActionResult(ret);
        }
        return finalResult;
    }

    /**
     * remove card check, need start thread when call this function
     */
    private void removeCard() {
        if (!mNeedRemoveCard) {
            if (isJVMA()) {
                backToMain();
                TransContext.clear();
            }
            return;
        }
        ActionRemoveCard actionRemoveCard = new ActionRemoveCard();
        if (isJVMA()) {
            actionRemoveCard.setEndListener((action, result) -> {
                backToMain();
                TransContext.clear();
            });
        }
        actionRemoveCard.execute(false);
    }

    private void backToMain() {
        if (mBackToMain) {
            ActivityStack.popAll();
        } else {
            if (ActivityStack.hasActivity(getMenuActivityClass())) {
                ActivityStack.popTo(getMenuActivityClass());
            } else {
                ActivityStack.popAll();
            }
        }
    }

    /**
     * override execute， add function to judge whether transaction check is running and add
     * transaction pre-deal
     */
    @Override
    public void execute() {
        initTrans(transType);
        PLog.i("start transaction: [ " + getCurrentTransName() + " ]");
        if (isTransRunning()) {
            setTransRunning(false);
            return;
        }
        //在交易执行时才生成默认的L21响应
        if (Trans.isPosLinkMode()) {
            PosLinkLastTransHandler.tryRememberDefaultLastTransL21Msg(
                    PosLinkMsgRememberHandler.getHeader(),
                    PosLinkMsgRememberHandler.getBody()
            );
        }
        setTransRunning(true);
        super.execute();
    }

    /**
     * save card information and input type after search card
     *
     * @param cardInfo  the card info
     * @param transData the trans data
     */
    protected void saveCardInfo(ActionSearchCard.CardInformation cardInfo, ITransData transData) {
        // manual input card number
        byte mode = cardInfo.getSearchMode();
        transData.setEmvTags(null);
        if (mode == ReaderTypeHelper.ICC || mode == ReaderTypeHelper.PICC) {
            INSTANCE.setNeedFallback(false);
            transData.setPanInputMode(mode == ReaderTypeHelper.ICC ? PanInputMode.CONTACT_IC
                    : PanInputMode.CONTACT_LESS_IC);
            transData.setJisType(TransData.JisType.JIS_1);
        } else {
            //track1 track2数据可能是本地保存的 在白名单获取add info时需要
            String track1 = cardInfo.getTrack1();
            String track2 = cardInfo.getTrack2();

            if (mode == ReaderTypeHelper.MANUAL) {
                transData.setKid(cardInfo.getKid());
                transData.setCardInfo(cardInfo.getPan(), cardInfo.getExpDate(), StringUtil.EMPTY);
                transData.setPanInputMode(PanInputMode.MANUAL);
                if (!TextUtils.isEmpty(track1) || !TextUtils.isEmpty(track2)) {
                    transData.setTrackData(track1, track2, null);
                }
            } else if (mode == ReaderTypeHelper.MAG) {
                transData.setPanInputMode(PanInputMode.MS_CONTACT);
                transData.setKid(cardInfo.getKid());
                if (track1.length() == 69 && track2.isEmpty()) {
                    transData.setJisType(TransData.JisType.JIS_2);
                    transData.setTrackData(track1, null, null);
                    transData.setCardInfo(cardInfo.getPan(),
                            TrackUtil.getJis2ExpDate(track1), StringUtil.EMPTY);
                } else {
                    transData.setJisType(TransData.JisType.JIS_1);
                    transData.setTrackData(null, track2, null);
                    transData.setCardInfo(cardInfo.getPan(), TrackUtil.getExpDate(track2),
                            TrackUtil.getHolderName(track1));
                }
                tryUpdateKidFromWhiteList();
            }
        }
    }

    /**
     * Goto state.
     *
     * @param state the state
     */
    @Override
    public void gotoState(TransactionState state) {

        super.gotoState(state);
    }

    /**
     * Research card.
     *
     * @param event          the event
     * @param targetActivity the target activity
     * @param targetState    the target state
     */
    protected void researchCard(DetectCardEvent event,
                                Class<? extends FragmentActivity> targetActivity, TransactionState targetState) {
        if (ActivityStack.hasActivity(targetActivity)) {
            ActivityStack.popTo(targetActivity);
            resetActionState(targetState);
            EventBusProvider.getEventBus().post(event);
        } else {
            transEnd(new ActionResult(TransResult.ERR_USER_CANCEL, event.mMessage));
        }
    }

    /**
     * Update sign.
     *
     * @param sign the sign
     */
    protected void updateSign(@Nullable byte[] sign) {
        mSignData = sign;
        if (sign != null) {
            ITransData transData = Trans.getCurrentTransData();
            if (transData instanceof TransData) {
                ((TransData) transData).setSignData(mSignData);
            }
            TransReceipt receipt = TransReceipt.from(mTransData);
            receipt.setSignData(sign);
            Repository.getInstance().insertOrReplaceTransReceipt(receipt);
        }
    }

    /**
     * Save contact ic info.
     *
     * @param transData the trans data
     */
    protected void saveContactICInfo(ITransData transData) {
        transData.setJisType(ITransData.JisType.JIS_1);
        transData.setPanInputMode(PanInputMode.CONTACT_IC);
    }

    /**
     * Is ic card detected boolean.
     *
     * @return the boolean
     */
    protected boolean isIcCardDetected() {
        try {
            if (!isPosLinkMode()) {
                PLog.d("pin_anr_test", "Trans isIcCardDetected before in thread: " + Thread.currentThread().getName());
                boolean res = NeptuneLiteProvider.getIcc().detect((byte) 0x01);
                PLog.d("pin_anr_test", "Trans isIcCardDetected after " + Thread.currentThread().getName());
                return res;
            }
            PLog.d("pin_anr_test", "Trans isIcCardDetected after " + Thread.currentThread().getName());
            return false;
        } catch (AGeneralException e) {
            //ignore exception
            PLog.d("pin_anr_test", e.getMessage() + " Trans isIcCardDetected after " + Thread.currentThread().getName());
            return false;
        }
    }

    public void saveOfflineData() {
        mTransData.setTransState(ITransData.TransStatus.O);
        Repository repo = Repository.getInstance();
        repo.insertTransData(mTransData);
        repo.increaseCreditTransNo();

        TransReceipt receipt = TransReceipt.from(mTransData);
        repo.insertOrReplaceTransReceipt(receipt);
        //离线数据保存成功后，尝试保存poslink报文，避免突然断电导致数据丢失
        trySaveLastSuccessTransPosLinkMsg();
    }

    /**
     * Generate print task print task.
     *
     * @param state the state
     * @return the print task
     */
    public PrintTask generatePrintTask(final TransactionState state) {
        return new PrintTask(
                transaction -> ((PrintTask) transaction).setTransReceipt(getTransReceipt()),
                PrintTask.genTransEndListener(this, state));
    }

    public PrintErrorTask generatePrintErrorTask(final TransactionState state,
                                                 boolean isPrintEnable) {
        return new PrintErrorTask(
                transaction -> ((PrintErrorTask) transaction).setTransReceipt(getTransReceipt()),
                PrintErrorTask.genTransEndListener(this, state), isPrintEnable);
    }

    private TransReceipt getTransReceipt() {
        TransReceipt receipt = LauncherSettings.isTrainingMode() ? getTrainingReceipt() :
                Repository.getInstance()
                        .findTransReceipt(mTransData.isUnionPay(), mTransData.getTransNo(),
                                getCurrentTransType() == TransType.CONTINUE_BILL);
        if (receipt == null) {
            receipt = TransReceipt.from(mTransData);
        }
        return receipt;
    }

    private TransReceipt getTrainingReceipt() {
        TransReceipt receipt = TransReceipt.from(mTransData);
        receipt.setSignData(mSignData);
        return LauncherSettings.isTrainingMode() ? TransReceipt.from(mTransData)
                : Repository.getInstance()
                .findTransReceipt(mTransData.isUnionPay(), mTransData.getTransNo(),
                        getCurrentTransType() == TransType.CONTINUE_BILL);
    }

    /**
     * Confirm jis 2 trans.
     *
     * @param confirmListener the confirm listener
     * @param cancelListener  the cancel listener
     */
    protected void confirmJis2Trans(PromptDialog.OnConfirmListener confirmListener,
                                    PromptDialog.OnCancelListener cancelListener) {
        DialogUtils.showConfirmDialog(getCurrentActivity(), getString(R.string.confirm_jis2_trans),
                confirmListener, cancelListener);
    }

    /**
     * Confirm use same card.
     *
     * @param confirmListener the confirm listener
     * @param cancelListener  the cancel listener
     */
    protected void confirmUseSameCard(PromptDialog.OnConfirmListener confirmListener,
                                      PromptDialog.OnCancelListener cancelListener) {
        if (!Utils.isUPT() &&        //追加是否是UPT设备的判断，如果是则不提示
                TransUtils.isPanSameWithLastTrans(mTransData.getTransType(), mTransData.getPan())) {
            PosLinkClientProvider.getPosLinkClient()
                    .setScreenId(Trans.requireCurrentTransData().isUnionPay()
                            ? ScreenID.UNION_PAY_ERR_SAME_CARD : ScreenID.CREDIT_ERR_SAME_CARD);
            DialogUtils.showConfirmDialog(getCurrentActivity(),
                    getString(R.string.same_pan_with_last_trans), confirmListener, cancelListener,
                    LauncherAppSettingsUseCase.getINSTANCE().getStandbyTimeout() > 0);
        } else {
            confirmListener.onConfirm();
        }
    }

    protected void tryAnotherInterface(TransactionState state) {
        if (isAllowFallback()) {
            researchCard(DetectCardEvent.disableReader(ReaderTypeHelper.PICC,
                        AndroidUtil.getString(R.string.try_another_interface),
                        AndroidUtil.getString(R.string.try_another_interface_en)), SearchCardActivity.class,
                state);
            return;
        }
        resetEntryPoint();
        researchCard(DetectCardEvent.error(
                        EEmvExceptions.EMV_ERR_DATA.getErrorCode(),
                        EEmvExceptions.EMV_ERR_DATA.getErrMsg(),
                        EEmvExceptions.EMV_ERR_DATA.getErrMsgEn()),
                SearchCardActivity.class, state);
    }

    protected boolean tryHandleNoAppClss(ActionResult result, TransactionState state) {
        Object data = result.getData();
        if (data == null) return false;

        boolean isTreatable = data == EEmvExceptions.EMV_ERR_NO_APP_CLSS;

        if (!isTreatable && (data instanceof CTransResult)) {
            ETransResult eTransResult = ((CTransResult) data).getTransResult();
            isTreatable = eTransResult == ETransResult.CLSS_OC_TRY_ANOTHER_INTERFACE;
        }

        if (!isTreatable) {
            isTreatable = data == EEmvExceptions.EMV_ERR_CLSS_USE_CONTACT;
        }

        if (!isTreatable) return false;

        ITransData currentTransData = Trans.getCurrentTransData();

        if (currentTransData == null) return false;

        PanInputMode panInputMode = currentTransData.getPanInputMode();
        boolean isPicc = panInputMode == PanInputMode.CONTACT_LESS_IC;
        if (!isPicc) return false;
        TransType transType = currentTransData.getTransType();
        int disableReader = -1;
        int disableSale = ReaderTypeHelper.PICC | ReaderTypeHelper.MAG | ReaderTypeHelper.MANUAL;
        int disableVoid = ReaderTypeHelper.PICC;
        if (transType == TransType.VOID) {
            disableReader = disableVoid;
        } else if (transType == TransType.SALE) {
            disableReader = disableSale;
        }
        if (disableReader == -1) return false;
        Repository repository = Repository.getInstance();
        int amount = currentTransData.getAmount();
        int defaultCvmRequiredLimit = repository.getDefaultCvmRequiredLimit();
        boolean isOverLimit = amount >= defaultCvmRequiredLimit;
        if (!isOverLimit) return false;
        //poslink 要求的支付方式
        byte posLinkSearchCardType = PosLinkUtil.tryGetPosLinkRequireSearchCardType();
        if (posLinkSearchCardType != -1) {
            //如果poslink指定了支付方式，需要判断是不是只有PICC
            //如果是只有PICC那么不允许切换
            if (posLinkSearchCardType == ReaderTypeHelper.PICC) return false;
        }
        currentTransData.setSupportFallback(false);
        if (ActivityStack.topIs(SearchCardActivity.class)) {
            DetectCardEvent detectCardEvent =
                    DetectCardEvent.disableReader(
                            (byte) disableReader,
                            AndroidUtil.getString(R.string.try_another_interface),
                            AndroidUtil.getString(R.string.try_another_interface_en));
            researchCard(detectCardEvent, SearchCardActivity.class, state);
        } else {
            transEnd(new ActionResult(TransResult.ERR_ICC_CMD));
        }
        return true;
    }

    protected boolean tryHandleAnotherInterface(ActionResult result, TransactionState state) {
        Object data = result.getData();
        if (data == null) {
            return false;
        }
        if (!(data instanceof CTransResult)) {
            return false;
        }
        ETransResult eTransResult = ((CTransResult) data).getTransResult();

        if (eTransResult != ETransResult.CLSS_OC_TRY_ANOTHER_INTERFACE) {
            return false;
        }
        resetEntryPoint();
        if (ActivityStack.topIs(SearchCardActivity.class)) {
            DetectCardEvent detectCardEvent =
                    DetectCardEvent.error(
                            EEmvExceptions.EMV_PICC_CMD_ERR.getErrorCode(),
                            AndroidUtil.getString(R.string.try_another_interface),
                            AndroidUtil.getString(R.string.try_another_interface_en));
            researchCard(detectCardEvent, SearchCardActivity.class, state);
        } else {
            transEnd(new ActionResult(TransResult.ERR_ICC_CMD));
        }
        return true;

    }


    protected void checkOneTimeUnsent() {
        Runnable task = takeCheckUnsentTask();
        if (task == null) return;
        task.run();
    }

    private void postCheckUnsentTask() {
        Runnable task = takeCheckUnsentTask();
        if (task == null) return;
        AppThreadExecutor.runOnWorkHandler(task);
    }

    @Nullable
    private Runnable takeCheckUnsentTask() {
        Runnable task = checkOneTimeUnsentTask;
        checkOneTimeUnsentTask = null;
        return task;
    }


}
