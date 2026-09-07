/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20220316  	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.executor;

import android.content.Context;
import android.content.SharedPreferences;
import android.text.TextUtils;

import com.google.gson.Gson;

import java.util.Map;
import java.util.Set;
import java.util.concurrent.TimeUnit;

import jp.gmofg.cps.database.LocalRepository;
import jp.gmofg.cps.https.Result;
import jp.gmofg.cps.R;
import jp.gmofg.cps.app.MyApp;
import jp.gmofg.cps.https.HttpsClient;
import jp.gmofg.cps.https.QrService;
import jp.gmofg.cps.https.Utils;
import jp.gmofg.cps.https.msg.Message;
import jp.gmofg.cps.https.msg.ResponseWithBaseData;
import jp.gmofg.cps.utils.Formatter;
import jp.gmofg.log.Logger;
import okhttp3.ResponseBody;
import retrofit2.HttpException;
import retrofit2.Response;
import rx.Observable;

public abstract class BaseNetUtil {

    private Context mContext;
    private SharedPreferences mSharedPreferences;

    protected QrService mQrService;
    protected String mTokenInRequest;
    protected LocalRepository mRepository;

    protected BaseNetUtil(){
        mContext = MyApp.getContext();
        mQrService = HttpsClient.getQrService();
        mSharedPreferences = MyApp.getSharedPreferences();
        mRepository = MyApp.getLocalRepository();
    }

    /**
     * get string from sharedPreference
     * @param stringId
     * @return
     */
    protected String getValue(int stringId){
        return getValue(mContext.getString(stringId));
    }

    /**
     * get string from sharedPreference
     * @param key
     * @return
     */
    private String getValue(String key){
        return mSharedPreferences.getString(key, "");
    }

    /**
     * save value into sharedPreference
     * @param map
     */
    protected void saveValues(Map<Integer, String> map){
        SharedPreferences.Editor editor = mSharedPreferences.edit();
        Set<Integer> keySet = map.keySet();
        for (int key : keySet){
            editor.putString(mContext.getString(key), map.get(key));
        }
        editor.apply();
    }

    /**
     * execute this runnable in some delay
     * @param runnable
     * @param delayMillis
     */
    protected void executeDelay(Runnable runnable, int delayMillis){
        Observable.timer(delayMillis, TimeUnit.MILLISECONDS)
                .subscribe(aLong -> runnable.run());
    }

    /**
     * check the sign in response
     * log this error if failed only
     * and don't do other operation
     * @param response
     */
    protected boolean checkSign(Message.Response response){
        String signInResponse = null;
        if (response.getBaseData() != null){
            signInResponse = response.getBaseData().getSign();
        }
        if (TextUtils.isEmpty(signInResponse)){
            Logger.error("checkSign there is no signInResponse");
            return false;
        }
        String signCalculated = Message.calcSignWithToken(response, mTokenInRequest);
        if (!TextUtils.equals(signCalculated, signInResponse)) {
            Logger.error("checkSign signCalculated: " + signCalculated);
            Logger.error("checkSign signInResponse: " + signInResponse);
            return false;
        }
        Logger.debug("checkSign signCalculated and signInResponse the same: " + signInResponse);
        return true;
    }


    /**
     * generate token in online request
     * @return nonce
     */
    protected String generateToken(int amount) {
        return generateToken(amount, Formatter.formatTime());
    }

    protected String generateToken(int amount, String timeStamp) {
        StringBuilder builder = new StringBuilder();
        String merchantId = Utils.getSpString(R.string.merchant_id);
        String terminalId = Utils.getSpString(R.string.terminal_id);
        String amountPad = String.format("%015d", amount);

        // 处理 merchantId 长度为12
        String formattedMerchantId;
        if (merchantId.length() < 12) {
            formattedMerchantId = String.format("%-12s", merchantId); // 右侧填充空格到12位
        } else {
            formattedMerchantId = merchantId.substring(0, 12); // 截取前12位
        }

        // 处理 terminalId 长度为13
        String formattedTerminalId;
        if (terminalId.length() < 13) {
            formattedTerminalId = String.format("%-13s", terminalId); // 右侧填充空格
        } else {
            formattedTerminalId = terminalId.substring(0, 13); // 截取前13位
        }
        builder.append(formattedMerchantId);
        builder.append(formattedTerminalId);
        builder.append(timeStamp);
        builder.append(amountPad);
        return builder.toString();
    }


    /**
     * 获取状态码400对应的错误信息
     * @param throwable
     * @return
     */
    protected Result.Error getErrorFromThrowable(Throwable throwable){
        if (throwable instanceof HttpException){
            HttpException httpException = (HttpException) throwable;
            Response response = httpException.response();
            if (response == null){
                return null;
            }
            ResponseBody responseBody = response.errorBody();
            if (responseBody == null){
                return null;
            }
            try {
                ResponseWithBaseData response1 = new Gson().fromJson(
                        responseBody.string(), ResponseWithBaseData.class);
                return new Result.Error(response1.getData().getErrorCode(), response1.getData().getErrorInfo())
                        .setSubErrorCode(response1.getData().getSubErrorCode());
            } catch (Exception e){//including IOException and JsonSyntaxException
                return null;
            }
        }
        return null;
    }

    protected TargetEnv getTargetEnv() {
        switch (getValue(R.string.target_env)) {
            case TargetEnv.ENV_PRODUCT_KEY: return TargetEnv.Product;
            case TargetEnv.ENV_OLD_STUB_KEY: return TargetEnv.Old_Stub;
            case TargetEnv.ENV_OLD_PRODUCT_KEY: return TargetEnv.Old_Product;
            default: return TargetEnv.Stub;
        }
    }
}
